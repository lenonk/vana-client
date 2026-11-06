//! Favourites in the modern layer (client/favs.rs keeps them and their rules): the star at a list row's right end
//! (gold when starred; a faint one on a hovered row) and the favourites strip, a small movable panel of the starred
//! spells, abilities and items that a click uses (a macro line's way: the target prompt where one is needed). Greyed
//! as their lists grey them; an item shows how many we carry. Hidden with /favs off, or while nothing is starred.

use super::super::World;
use super::super::favs::{self, Fav};
use super::super::magic::{self, Kind};
use super::{Panels, Title, paint};
use egui::{Align2, Color32, FontId, Rect, Sense, Ui, pos2, vec2};

/// The room a row keeps at its right for the star.
pub const STAR_W: f32 = 30.0;

/// The star at the right end of row `r`; true when it was clicked.
pub fn star(ui: &Ui, m: &super::Modern, r: Rect, on: bool, row_hovered: bool) -> bool {
    let at = Rect::from_center_size(pos2(r.max.x - STAR_W / 2.0, r.center().y), vec2(STAR_W - 4.0, r.height() - 6.0));
    let resp = ui.interact(at, ui.id().with(("fav star", r.min.y as i32)), Sense::click());
    let colour = match (on, resp.hovered()) {
        (true, _) => m.theme.color("gold"),
        (false, true) => m.theme.color("gold").gamma_multiply(0.7),
        (false, false) if row_hovered => m.theme.color("muted").gamma_multiply(0.6),
        _ => Color32::TRANSPARENT,
    };
    if colour != Color32::TRANSPARENT {
        ui.painter().add(paint::star(at.center(), 7.5, colour));
    }
    if resp.hovered() {
        m.tip_with(ui, at, || ((if on { "Remove from Favourites" } else { "Add to Favourites" }).to_string(), "Ctrl+F stars the cursor's row.".to_string()));
    }
    resp.clicked()
}

/// One favourite as the strip shows it.
pub(super) struct Chip {
    pub fav: Fav,
    pub name: String,
    pub icon: Option<egui::TextureId>,
    pub off: bool,
    pub count: Option<u32>,
}

fn chips(p: &Panels, w: &World) -> Vec<Chip> {
    let m = p.m;
    let game_icon = |kind: Kind, id: u16| p.gu.and_then(|gu| magic::row_icon(w, kind, id).and_then(|(lib, k)| m.sprite_icon(p.ctx, gu, lib, k)));
    favs::all(w)
        .into_iter()
        .map(|fav| match fav {
            Fav::Spell(id) => Chip { fav, name: favs::name(w, fav), icon: game_icon(Kind::Magic, id), off: super::lists::off(w, Kind::Magic, id), count: None },
            Fav::Ability(id) => Chip { fav, name: favs::name(w, fav), icon: game_icon(Kind::Ability, id), off: super::lists::off(w, Kind::Ability, id), count: None },
            Fav::Item(id) => {
                let (name, n) = favs::item(w, id);
                let icon = w.item_info.as_ref().and_then(|info| m.item_icon(p.ctx, info, id));
                Chip { fav, name, icon, off: n == 0, count: Some(n) }
            }
        })
        .collect()
}

/// The strip: a tile per favourite (its icon, an item's count), its name as the tooltip; a click uses it.
pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.fav_strip || w.favourites.is_empty() || super::super::event::active(w) {
        return;
    }
    let chips = chips(p, w);
    if chips.is_empty() {
        return;
    }
    if let Some(f) = strip(p, &chips) {
        favs::run(w, f);
    }
}

/// The strip's panel; the favourite clicked.
pub(super) fn strip(p: &mut Panels, chips: &[Chip]) -> Option<Fav> {
    const TILE: f32 = 40.0;
    const PER_ROW: usize = 8;
    let m = p.m;
    let width = (chips.len().min(PER_ROW) as f32) * (TILE + 4.0) + 40.0;
    let mut used = None;
    p.panel("favourites", Some(Title { icon: None, text: "Favourites", note: None }), width.max(180.0), |ui| {
        for line in chips.chunks(PER_ROW) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for c in line {
                    let (r, resp) = ui.allocate_exact_size(vec2(TILE, TILE), Sense::click());
                    let edge = if resp.hovered() && !c.off { m.theme.color("gold") } else { m.theme.color("bronze").gamma_multiply(0.7) };
                    ui.painter().rect_filled(r, 5.0, m.theme.color("charcoal"));
                    ui.painter().rect_stroke(r, 5.0, egui::Stroke::new(1.0, edge), egui::StrokeKind::Inside);
                    let tint = if c.off { Color32::from_gray(100) } else { Color32::WHITE };
                    let icon = Rect::from_center_size(r.center(), vec2(28.0, 28.0));
                    match c.icon {
                        Some(t) => {
                            ui.painter().image(t, icon, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
                        }
                        // No art: the name's first letters.
                        None => {
                            let short: String = c.name.chars().take(2).collect();
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, short, FontId::proportional(m.theme.ui), if c.off { m.theme.color("muted") } else { m.theme.color("text") });
                        }
                    }
                    if let Some(n) = c.count {
                        paint::outlined(ui.painter(), pos2(r.max.x - 3.0, r.max.y - 2.0), Align2::RIGHT_BOTTOM, &n.to_string(), FontId::proportional(m.theme.small - 2.0), m.theme.color("text"));
                    }
                    m.tip_with(ui, r, || (c.name.clone(), "Click to use. Unstar it in its list.".to_string()));
                    if resp.clicked() && !c.off {
                        used = Some(c.fav);
                    }
                }
            });
        }
    });
    used
}
