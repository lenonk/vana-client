//! The modern Status page in the command window (client/status.rs keeps the state and its rules): the persona, the
//! level and its gauge, retail's rows on the left and beside them the page under the cursor at once (retail shows one
//! after Enter): profile, job and master levels, combat, magic and craft skills, and Currencies 1 and 2 as one list.
//! Unity opens retail's window over it; Merit Points, Job Points and Alter Ego Points open modern/points.rs.

use super::super::status::{self, STATCOM};
use super::super::{World, currencies, items as gitems, monstrosity};
use super::{Panels, Title, equip, hud, paint};
use crate::audio::Ui as Se;
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, pos2, vec2};

/// statcom2's children top to bottom with Currencies 2 merged into Currencies (the numbered entries merged).
const ROWS: [(u16, &str); 12] = [
    (1, "Profile"),
    (2, "Job Levels"),
    (12, "Master Levels"),
    (3, "Combat Skills"),
    (4, "Magic Skills"),
    (5, "Craft Skills"),
    (6, "Currencies"),
    (11, "Unity"),
    (9, "Play Time"),
    (7, "Merit Points"),
    (8, "Job Points"),
    (13, "Alter Ego Points"),
];
const SIDE_W: f32 = 190.0;
const PAGE_W: f32 = 400.0;
const ROW_H: f32 = 30.0;
const LINE: f32 = 24.0;

/// The page shows: Status open with none of its own windows over it.
fn shown(w: &World) -> bool {
    w.modern && w.status.is_some() && w.currencies.is_none() && w.unity.is_none() && w.merits_ui.is_none() && w.jp_ui.is_none() && w.ae_ui.is_none()
}

fn cursor_row(sel: u16) -> usize {
    ROWS.iter().position(|r| r.0 == sel || (sel == 10 && r.0 == 6)).unwrap_or(0)
}

fn moved_to(w: &mut World, k: usize) {
    if let Some(s) = w.status.as_mut() {
        s.sel = ROWS[k].0;
    }
    status::show_page(w);
    if ROWS[k].0 == 6 {
        currencies::ask_both(w);
    }
}

/// Up / Down along the rows (wrapping), Left / Right a page of the page, Enter the row (its window or command), Escape
/// closes Status. True when the page took the key.
pub fn key(w: &mut World, key: egui::Key) -> bool {
    use egui::Key as K;
    if !shown(w) || w.confirm.is_some() {
        return false;
    }
    let Some(s) = w.status.as_ref() else { return false };
    let k = cursor_row(s.sel);
    match key {
        K::ArrowUp | K::ArrowDown => {
            let n = ROWS.len();
            moved_to(w, if key == K::ArrowDown { (k + 1) % n } else { (k + n - 1) % n });
            w.sounds.push(Se::Cursor as u32);
        }
        K::ArrowLeft | K::ArrowRight => {
            if let Some(s) = w.status.as_mut() {
                s.m_page += if key == K::ArrowRight { 1 } else { -1 };
            }
        }
        K::Enter => status::enter(w),
        K::Escape => {
            w.sounds.push(Se::Cancel as u32);
            status::close(w);
        }
        _ => return false,
    }
    true
}

/// A row of the page: the label at the left, the value at the right.
fn pair(ui: &mut Ui, m: &super::Modern, label: &str, value: &str, colour: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), LINE), Sense::hover());
    ui.painter().text(pos2(r.min.x + 4.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(m.theme.small), m.theme.color("muted"));
    ui.painter().text(pos2(r.max.x - 4.0, r.center().y), Align2::RIGHT_CENTER, value, FontId::proportional(m.theme.ui), colour);
}

fn heading(ui: &mut Ui, m: &super::Modern, text: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(text).size(m.theme.small).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("gold")));
    ui.add_space(2.0);
}

/// Cells two a row (jobs, skills): an optional icon, the name, the value right-aligned in its colour.
fn cells(ui: &mut Ui, m: &super::Modern, items: &[Cell], icons: &[Option<super::theme::Sprite>]) {
    let half = (ui.available_width() - 12.0) / 2.0;
    for (k, chunk) in items.chunks(2).enumerate() {
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), LINE + 4.0), Sense::hover());
        for (j, (name, nc, value, vc)) in chunk.iter().enumerate() {
            let r = Rect::from_min_size(pos2(row.min.x + j as f32 * (half + 12.0), row.min.y), vec2(half, LINE));
            ui.painter().rect_filled(r, 4.0, m.theme.color("charcoal").gamma_multiply(0.6));
            let mut x = r.min.x + 6.0;
            if let Some(s) = icons.get(k * 2 + j).cloned().flatten() {
                ui.painter().add(paint::image(&s, pos2(x, r.center().y - 9.0), 18.0, Color32::WHITE));
                x += 24.0;
            }
            ui.painter().text(pos2(x, r.center().y), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.small), *nc);
            ui.painter().text(pos2(r.max.x - 6.0, r.center().y), Align2::RIGHT_CENTER, value, FontId::monospace(m.theme.small), *vc);
        }
    }
}

/// A cell: the name and its colour, the value and its colour.
type Cell = (String, Color32, String, Color32);

pub fn draw(p: &mut Panels, w: &mut World) {
    if !shown(w) {
        return;
    }
    let Some(gu) = p.gu else { return };
    // Currencies under the cursor (opened there too): both lists asked for once.
    if w.status.as_ref().is_some_and(|s| matches!(s.sel, 6 | 10)) {
        currencies::ask_both(w);
    }
    let Some(s) = w.status.clone() else { return };
    let m = p.m;
    let ctx = p.ctx;
    let mut persona = gitems::persona(w);
    if monstrosity::active(w) {
        persona.name = w.my_mon_name.as_ref().map_or(String::new(), |n| n.1.clone());
    }
    let stats = w.stats.clone().unwrap_or_default();
    let ready = w.recasts_seen && w.stats.is_some();
    let sep = gu.lobby_msg(0xc, 0x5b, ",");
    let row = cursor_row(s.sel);
    let sel = ROWS[row].0;
    let white = m.theme.color("text");
    let (gold, blue, muted) = (m.theme.color("gold"), m.theme.color("focus"), m.theme.color("muted"));

    // The level: retail's level window as a gauge (the master level once one is unlocked).
    let ml = stats.ml.1 & 1 != 0;
    let capped = if ml { stats.ml.1 & 2 != 0 && stats.ml_exp.0 >= stats.ml_exp.1.wrapping_sub(1) } else { w.merits.is_some_and(|x| x.1 & 0x4000 != 0) };
    let (head, level, now, next) = if ml {
        (gu.lobby_msg(0xc, 0x203, "M. Level"), stats.ml.0, stats.ml_exp.0 as i64, stats.ml_exp.1 as i64)
    } else {
        (gu.lobby_msg(0xc, 0x56, "Level"), w.real_level.unwrap_or(stats.main.1), stats.exp.0 as i64, stats.exp.1 as i64)
    };
    let exp_label = if ml { gu.lobby_msg(0xc, 0x204, "") } else { gu.lobby_msg(0xc, 0x57, "EXP") };

    // The page under the cursor, gathered before drawing.
    let caption = |lib: &str, index: usize| m.caption(gu, lib, index);
    let (page_cells, page_icons): (Vec<Cell>, Vec<Option<super::theme::Sprite>>) = match sel {
        2 | 12 => {
            let rows = status::job_rows(w, sel == 12);
            let cells = rows
                .iter()
                .map(|&(job, lv)| {
                    let name = caption("menu    windowps", 212 + job as usize).unwrap_or_else(|| hud::job(job).to_string());
                    let c = if job == stats.main.0 { gold } else if job == stats.sub.0 { blue } else { white };
                    (name, c, lv.to_string(), white)
                })
                .collect();
            (cells, rows.iter().map(|&(job, _)| hud::job_icon(job).and_then(|id| m.art.sprite(&id))).collect())
        }
        3 | 4 => {
            let cells = status::skill_rows(w, sel == 4)
                .into_iter()
                .map(|(id, v)| (caption("menu    keytops3", 208 + id).unwrap_or_default(), white, (v & 0x7fff).to_string(), if v & 0x8000 != 0 { blue } else { white }))
                .collect();
            (cells, Vec::new())
        }
        _ => (Vec::new(), Vec::new()),
    };
    let crafts: Vec<(String, String, Color32, String)> = status::craft_rows(w)
        .into_iter()
        .map(|(id, v)| {
            let rank = if v & 0x1f < 0x10 { caption("menu    keytops3", 193 + (v & 0x1f) as usize).unwrap_or_default() } else { String::new() };
            (caption("menu    keytops3", 208 + id).unwrap_or_default(), rank, if v & 0x8000 != 0 { blue } else { white }, ((v >> 5) & 0x3ff).to_string())
        })
        .collect();
    let money = if sel == 6 { currencies::merged(gu, &s.cur) } else { Vec::new() };
    let title = w.names.as_ref().and_then(|n| {
        use crate::dat::names::NameSource;
        n.title(stats.title as u32)
    });
    let nation = (stats.nation < 3).then(|| caption("menu    keytops3", 74 + stats.nation as usize)).flatten();
    let home = crate::dat::font::plain(&crate::dat::font::decode_sjis(&gu.area_name(stats.home)));
    let race = status::RACES.get(w.my_race as usize).copied().unwrap_or("");
    let mon = w.monstrosity.filter(|x| x.0 & 0x18 == 0x18);
    let (mentor, mastery) = w.job_info.map_or((0, 0), |j| (j.mentor, j.mastery));
    let help = gu.focus_text(STATCOM, sel)[0].clone().unwrap_or_default();
    let mut held_only = m.held_only.get();

    let moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let (mut hover, mut click) = (None, None);
    let mut open = false;
    p.panel("window", Some(Title { icon: Some("icons/status"), text: "Status", note: None }), SIDE_W + 18.0 + PAGE_W, |ui| {
        equip::persona_row(ui, m, &persona, &stats);
        ui.add_space(6.0);
        if ready {
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 33.0), Sense::hover());
            let bar = Rect::from_min_max(pos2(r.min.x, r.max.y - hud::BAR), r.max);
            let f = if next > 0 { (now as f32 / next as f32).min(1.0) } else { 0.0 };
            let value = format!("{} / {}", status::grouped(now, &sep), status::grouped(next, &sep));
            let c = if capped { blue } else { white };
            hud::labelled_bar(ui, m, "experience", bar, f, (&format!("{head} {level}   {exp_label}"), c), (&value, c));
        }
        ui.add_space(10.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(SIDE_W);
                for (k, (id, label)) in ROWS.iter().enumerate() {
                    let (r, resp) = ui.allocate_exact_size(vec2(SIDE_W, ROW_H), Sense::click());
                    let off = s.off.contains(id) || (*id == 6 && s.off.contains(&10) && s.off.contains(&6));
                    let face = match () {
                        _ if k == row => "controls/list_row_selected",
                        _ if off => "controls/list_row_disabled",
                        _ if resp.hovered() => "controls/list_row_hover",
                        _ => "controls/list_row_normal",
                    };
                    super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
                    ui.painter().text(pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, *label, FontId::proportional(m.theme.ui), if off { muted } else { white });
                    if resp.hovered() && moved {
                        hover = Some(k);
                    }
                    if resp.clicked() {
                        click = Some(k);
                    }
                    ui.add_space(2.0);
                }
            });
            ui.add_space(18.0);
            // The page as tall as it needs, up to most of the screen; at least the list beside it.
            let height = (ui.ctx().content_rect().height() * 0.7).max(ROWS.len() as f32 * (ROW_H + 2.0));
            let mut area = egui::ScrollArea::vertical().id_salt("status page").max_height(height).min_scrolled_height(height).auto_shrink([false, true]);
            if s.m_page != 0 {
                let off = ui.ctx().data(|d| d.get_temp::<f32>(egui::Id::unique("status page off"))).unwrap_or(0.0);
                area = area.vertical_scroll_offset((off + s.m_page as f32 * (height - LINE)).max(0.0));
            }
            let out = area.show(ui, |ui| ui.vertical(|ui| {
                ui.set_width(PAGE_W);
                ui.label(egui::RichText::new(ROWS[row].1).size(m.theme.title).family(FontFamily::Name(super::STRONG.into())).color(gold));
                ui.add_space(4.0);
                if !ready {
                    ui.label(egui::RichText::new("Waiting for the status...").size(m.theme.small).color(muted));
                    return;
                }
                match sel {
                    1 => {
                        pair(ui, m, "Title", title.as_deref().unwrap_or(""), white);
                        pair(ui, m, "Nation", nation.as_deref().unwrap_or("--"), white);
                        pair(ui, m, "Race", race, white);
                        pair(ui, m, "Home Point", &home, white);
                        pair(ui, m, "Rank", &stats.rank.to_string(), white);
                        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
                        let bar = Rect::from_min_max(pos2(r.min.x, r.max.y - hud::BAR), r.max);
                        hud::labelled_bar(ui, m, "experience", bar, stats.rank_points as f32 / 4096.0, ("Rank Points", white), ("", white));
                        if stats.ilvl != 0 {
                            pair(ui, m, "Item Level", &format!("{}  Su{}", stats.ilvl as u32 + 99, stats.su), white);
                        }
                        heading(ui, m, "Attributes");
                        pair(ui, m, "Max HP", &stats.hp_max.to_string(), if persona.enc & 1 << 0x17 != 0 { m.theme.color("hp") } else { white });
                        pair(ui, m, "Max MP", &stats.mp_max.to_string(), if persona.enc & 1 << 0x18 != 0 { m.theme.color("hp") } else { white });
                        for (k, (base, bonus)) in persona.stats.iter().enumerate() {
                            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), LINE), Sense::hover());
                            ui.painter().text(pos2(r.min.x + 4.0, r.center().y), Align2::LEFT_CENTER, ["STR", "DEX", "VIT", "AGI", "INT", "MND", "CHR"][k], FontId::proportional(m.theme.small), muted);
                            let mut job = egui::text::LayoutJob::default();
                            let red = persona.enc & 1 << (0x10 + k) != 0;
                            job.append(base.trim(), 0.0, egui::TextFormat::simple(FontId::proportional(m.theme.ui), if red { m.theme.color("hp") } else { white }));
                            if let Some((b, c)) = bonus {
                                job.append(b, 0.0, egui::TextFormat::simple(FontId::proportional(m.theme.ui), super::chat::colour(c.to_le_bytes())));
                            }
                            let g = ui.painter().layout_job(job);
                            ui.painter().galley(pos2(r.max.x - 4.0 - g.size().x, r.center().y - g.size().y / 2.0), g, Color32::WHITE);
                        }
                        if let Some((_, rank, prestige)) = mon {
                            heading(ui, m, "Monstrosity");
                            pair(ui, m, "Gladiator Rank", ["Gladiator", "Colosseum Gladiator", "High Colosseum Gladiator"].get((rank >> 2 & 3) as usize).copied().unwrap_or(""), white);
                            pair(ui, m, "Prestige", &prestige.to_string(), white);
                            pair(ui, m, "Mastery Rank", &mastery.to_string(), white);
                            pair(ui, m, "Mentor", &if mentor > 0 { format!("Mentor {mentor}") } else { "--".into() }, white);
                        }
                    }
                    2..=4 | 12 => cells(ui, m, &page_cells, &page_icons),
                    5 => {
                        for (name, rank, rc, lv) in &crafts {
                            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), LINE + 4.0), Sense::hover());
                            let r = Rect::from_min_size(r.min, vec2(r.width(), LINE));
                            ui.painter().rect_filled(r, 4.0, m.theme.color("charcoal").gamma_multiply(0.6));
                            ui.painter().text(pos2(r.min.x + 6.0, r.center().y), Align2::LEFT_CENTER, name, FontId::proportional(m.theme.small), white);
                            ui.painter().text(pos2(r.min.x + 170.0, r.center().y), Align2::LEFT_CENTER, rank, FontId::proportional(m.theme.small), *rc);
                            ui.painter().text(pos2(r.max.x - 6.0, r.center().y), Align2::RIGHT_CENTER, lv, FontId::monospace(m.theme.small), white);
                        }
                    }
                    6 => {
                        if s.cur.iter().all(|c| c.is_none()) {
                            ui.label(egui::RichText::new("Asking for the currencies...").size(m.theme.small).color(muted));
                            return;
                        }
                        ui.checkbox(&mut held_only, egui::RichText::new("Only those held").size(m.theme.small).color(white));
                        ui.add_space(4.0);
                        for (label, v, tip) in money.iter().filter(|x| !held_only || x.1 != 0) {
                            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), LINE), Sense::hover());
                            let c = if *v == 0 { muted } else { white };
                            ui.painter().text(pos2(r.min.x + 4.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(m.theme.small), c);
                            ui.painter().text(pos2(r.max.x - 4.0, r.center().y), Align2::RIGHT_CENTER, status::grouped(*v, &sep), FontId::monospace(m.theme.small), c);
                            if !tip.is_empty() {
                                resp.on_hover_text(tip);
                            }
                        }
                    }
                    _ => {
                        ui.label(egui::RichText::new(&help).size(m.theme.small).color(white));
                        ui.add_space(10.0);
                        let off = s.off.contains(&sel);
                        let what = if sel == 9 { "Show" } else { "Open" };
                        if ui.add_enabled(!off, egui::Button::new(egui::RichText::new(what).size(m.theme.ui))).clicked() {
                            open = true;
                        }
                        if off {
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("Not available to this character yet.").size(m.theme.small).color(muted));
                        }
                    }
                }
            }));
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("status page off"), out.state.clamped_offset().y));
        });
    });
    m.held_only.set(held_only);
    if let Some(st) = w.status.as_mut() {
        st.m_page = 0;
    }
    if let Some(k) = hover.filter(|&k| k != row) {
        moved_to(w, k);
        w.sounds.push(Se::Cursor as u32);
    }
    if let Some(k) = click {
        if k != row {
            moved_to(w, k);
        }
        status::enter(w);
    }
    if open {
        status::enter(w);
    }
}
