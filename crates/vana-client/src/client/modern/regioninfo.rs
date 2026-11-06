//! Region Info's map windows (client/region.rs keeps the map, its markers and the rules): what retail's conquer1,
//! conq6sta, beseige1 / beseige2, coloresu, colorank and the legend windows show, as one panel at the screen's edge
//! away from the cursor. Retail's label sprites are named by their captions (fallbacks in English).

use super::super::{World, region};
use super::{Panels, Title};
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Ui, pos2, vec2};

const W: f32 = 300.0;
const LINE: f32 = 24.0;
/// framesus 73 / 75 / 74 / 94 / 93 (region.rs LABELS).
const NATIONS: [(usize, &str); 5] = [(73, "San d'Oria"), (75, "Bastok"), (74, "Windurst"), (94, "Beastmen"), (93, "Neutral")];
/// keytops3 81 / 80 / 78 / 77 / 79 (region.rs LEVELS).
const LEVELS: [(usize, &str); 5] = [(81, "Minimal"), (80, "Minor"), (78, "Major"), (77, "Dominant"), (79, "Equal")];
const LEGENDS: [&str; 4] = ["Conquest Points", "Imperial Standing", "Allied Notes", "Bayld"];
/// colorank's rows *(inf: the Coalition list's order; retail draws them as one label sprite)*.
const COALITIONS: [&str; 6] = ["Pioneers", "Mummers", "Inventors", "Peacekeepers", "Scouts", "Couriers"];

fn caption(p: &Panels, lib: &str, index: usize, fallback: &str) -> String {
    p.gu.and_then(|gu| p.m.caption(gu, lib, index)).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| fallback.to_string())
}

fn heading(ui: &mut Ui, m: &super::Modern, text: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(text).size(m.theme.small).family(FontFamily::Name(super::STRONG.into())).color(m.theme.color("gold")));
}

fn pair(ui: &mut Ui, m: &super::Modern, label: &str, value: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), LINE), Sense::hover());
    ui.painter().text(pos2(r.min.x + 4.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(m.theme.small), m.theme.color("muted"));
    ui.painter().text(pos2(r.max.x - 4.0, r.center().y), Align2::RIGHT_CENTER, value, FontId::monospace(m.theme.small), m.theme.color("text"));
}

/// A labelled bar (0..1), `lit` the one ahead.
fn gauge(ui: &mut Ui, m: &super::Modern, label: &str, f: f32, lit: bool, value: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), LINE + 8.0), Sense::hover());
    let c = if lit { m.theme.color("gold") } else { m.theme.color("text") };
    ui.painter().text(pos2(r.min.x + 4.0, r.min.y + 8.0), Align2::LEFT_CENTER, label, FontId::proportional(m.theme.small), c);
    ui.painter().text(pos2(r.max.x - 4.0, r.min.y + 8.0), Align2::RIGHT_CENTER, value, FontId::monospace(m.theme.small), c);
    let track = Rect::from_min_max(pos2(r.min.x + 4.0, r.max.y - 10.0), pos2(r.max.x - 4.0, r.max.y - 4.0));
    ui.painter().rect_filled(track, 3.0, m.theme.color("charcoal"));
    let fill = Rect::from_min_max(track.min, pos2(track.min.x + track.width() * f.clamp(0.0, 1.0), track.max.y));
    ui.painter().rect_filled(fill, 3.0, if lit { m.theme.color("gold") } else { m.theme.color("bronze") });
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    let (Some(v), Some(gu)) = (region::info_view(w), p.gu) else { return };
    if v.region.is_none() && v.ranking.is_none() && v.besieged.is_none() && v.stronghold.is_none() && v.colony.is_none() && v.coalitions.is_none() && v.legend.is_none() {
        return;
    }
    let m = p.m;
    let nation = |k: usize| caption(p, "menu    framesus", NATIONS[k.min(4)].0, NATIONS[k.min(4)].1);
    let level = |k: usize| caption(p, "menu    keytops3", LEVELS[k.min(4)].0, LEVELS[k.min(4)].1);
    let army = |k: usize| caption(p, "menu    keytops3", 367 + k, ["Mamool Ja Savages", "Troll Mercenaries", "Undead Swarm"][k.min(2)]);
    let orders = |o: u32| if o < 6 { caption(p, "menu    keytops3", 377 + o as usize, &format!("Orders {o}")) } else { String::new() };
    let title = region::map_title(gu, v.mode);
    // Built before the panel borrows p.
    let mut lines: Vec<Box<dyn Fn(&mut Ui)>> = Vec::new();
    if let Some(r) = &v.region {
        let owner = nation(r.owner as usize);
        let tally = r.tally.clone();
        let rows: Vec<(String, f32, bool, String)> = if r.beast_flag && !r.precise {
            Vec::new()
        } else {
            (0..if r.precise { 4 } else { 3 })
                .map(|k| {
                    let lit = r.precise && k == r.top as usize;
                    if r.precise {
                        let f = if k < 3 { r.levels[k] } else { r.beast };
                        (nation(k), f, lit, format!("{:.0}%", f * 100.0))
                    } else {
                        let l = r.levels[k] as usize;
                        (nation(k), (l + 1) as f32 / 4.0, false, if l < 5 { level(l) } else { String::new() })
                    }
                })
                .collect()
        };
        let dominant = (r.beast_flag && !r.precise).then(|| caption(p, "menu    framesus", 95, "The beastmen are dominant."));
        lines.push(Box::new(move |ui: &mut Ui| {
            heading(ui, m, "Region");
            pair(ui, m, "Controlled by", &owner);
            pair(ui, m, "Tally", &tally);
            if let Some(d) = &dominant {
                ui.label(egui::RichText::new(d).size(m.theme.small).color(m.theme.color("hp")));
            }
            for (name, f, lit, value) in &rows {
                gauge(ui, m, name, *f, *lit, value);
            }
        }));
    }
    if let Some((ranks, alliance)) = &v.ranking {
        let rows: Vec<(String, String)> = ranks.iter().map(|&(r, k)| (format!("{r}."), nation(k))).collect();
        let alliance = alliance.then(|| caption(p, "menu    keytops3", 119, "Alliance"));
        lines.push(Box::new(move |ui: &mut Ui| {
            heading(ui, m, "Nation Ranking");
            for (r, n) in &rows {
                pair(ui, m, r, n);
            }
            if let Some(a) = &alliance {
                ui.label(egui::RichText::new(a).size(m.theme.small).color(m.theme.color("muted")));
            }
        }));
    }
    if let Some((zahbi, armies)) = v.besieged {
        let own = caption(p, "menu    keytops3", 370 + zahbi as usize, "Al Zahbi");
        let rows: Vec<(String, u32, String)> = armies.iter().enumerate().map(|(k, &(lv, o))| (army(k), lv, orders(o))).collect();
        lines.push(Box::new(move |ui: &mut Ui| {
            heading(ui, m, "Besieged");
            pair(ui, m, "Al Zahbi", &own);
            for (name, lv, o) in &rows {
                pair(ui, m, name, &format!("Lv {lv}"));
                if !o.is_empty() {
                    pair(ui, m, "", o);
                }
            }
        }));
    }
    if let Some((k, forces, (lv, o), mirrors, prisoners)) = v.stronghold {
        let name = caption(p, "menu    keytops3", 374 + k, ["Mamook", "Halvung", "Arrapago Reef"][k.min(2)]);
        let (army, orders) = (army(k), orders(o));
        lines.push(Box::new(move |ui: &mut Ui| {
            heading(ui, m, &name);
            let f = if forces < 100 { forces as f32 / 100.0 } else { ((forces - 100) % 101) as f32 / 100.0 };
            gauge(ui, m, "Forces", f, forces >= 100, &forces.to_string());
            pair(ui, m, &army, &format!("Lv {lv}"));
            if !orders.is_empty() {
                pair(ui, m, "Orders", &orders);
            }
            pair(ui, m, "Archaic Mirrors", &mirrors.to_string());
            pair(ui, m, "Prisoners", &prisoners.to_string());
        }));
    }
    if let Some((rate, biv, max)) = v.colony {
        lines.push(Box::new(move |ui: &mut Ui| {
            heading(ui, m, "Colonization");
            gauge(ui, m, "Development", rate as f32 / 100.0, false, &format!("{rate}%"));
            pair(ui, m, "Bivouacs", &format!("{biv}/{max}"));
        }));
    }
    if let Some(ranks) = v.coalitions {
        lines.push(Box::new(move |ui: &mut Ui| {
            heading(ui, m, "Coalition Ranks");
            for (k, r) in ranks.iter().enumerate() {
                pair(ui, m, COALITIONS[k], &r.to_string());
            }
        }));
    }
    if let Some(value) = v.legend {
        let label = LEGENDS[v.mode.min(3)];
        lines.push(Box::new(move |ui: &mut Ui| {
            ui.add_space(6.0);
            pair(ui, m, label, &value.to_string());
        }));
    }
    let screen = p.ctx.content_rect();
    let x = if v.left { 16.0 } else { screen.width() - W - 44.0 - 16.0 };
    p.panel_at("region info", Some(Title { icon: Some("icons/map"), text: &title, note: None }), W + 44.0, pos2(x, screen.height() * 0.12), |ui| {
        ui.set_width(W);
        for f in &lines {
            f(ui);
        }
    });
}
