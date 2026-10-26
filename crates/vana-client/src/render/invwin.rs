//! Inventory, equipment, status and gil windows (39542 menus "inventor", "equip", "persona",
//! "money"), drawn from the models in inventory.rs at the offsets the client's draw functions use.

use super::screens::{push_shape_text, push_shape_text_right, push_window, shape_text_width};
use super::ui::UiBatch;
use crate::dat::font::{Font, encode_sjis};
use crate::dat::ui::{Menu, SpriteLib, UiFile};
use crate::inventory::{EQUIP_FIELDS, Equipment, ItemList, Persona, ROW_PITCH};

/// Texture name an item icon is loaded under.
pub fn icon_texture(id: u16) -> String {
    format!("\0icon {id}")
}

/// Game-font text; `tint` is the ARGB word (quads take BGRA).
fn text(b: &mut UiBatch, font: &Font, atlas: &str, s: &str, x: f32, y: f32, tint: u32) -> f32 {
    if b.modern.is_some() {
        return b.push_mtext(s, super::mtext::Face::Regular, super::mtext::TEXT_EM, x, y + super::mtext::TEXT_DY, super::screens::argb(tint), false);
    }
    let mut quads = Vec::new();
    let end = font.layout_line(&encode_sjis(s), x.round() as i32, y.round() as i32, tint.to_le_bytes(), [1.0, 1.0], &mut quads);
    b.push_text(&quads, atlas);
    end as f32
}

/// Kind-0 sprite refs of the children (cursor stops) at their positions, as the menu draws them.
pub fn push_children(b: &mut UiBatch, ui: &UiFile, m: &Menu, wx: f32, wy: f32) {
    for e in m.elements.iter().skip(1) {
        for r in e.refs.iter().filter(|r| r.kind == 0) {
            if let Some(lib) = ui.lib(&r.lib) {
                b.push_sprite(ui, lib, r.index as usize, wx + e.x as f32, wy + e.y as f32, 1.0, 1.0, 0x8080_8080, 0);
            }
        }
    }
}

/// The hand cursor: the window's kind-6 library (anc_item, anc_equi), frame `frame`, at child `child`.
pub fn push_cursor(b: &mut UiBatch, ui: &UiFile, m: &Menu, wx: f32, wy: f32, child: usize, frame: usize) {
    let Some(r) = m.elements[0].refs.iter().find(|r| r.kind == 6) else { return };
    let (Some(lib), Some(e)) = (ui.lib(&r.lib), m.elements.get(child)) else { return };
    b.push_sprite(ui, lib, frame % lib.sprites.len().max(1), wx + e.x as f32, wy + e.y as f32, 1.0, 1.0, 0x8080_8080, 0);
}

/// The item list (FUN_101edb70 with FUN_101ebda0 cells). Rows start at window + (3, 4), 18 px
/// apart; US columns (0x10375ee4) put the icon at 0 (32x32 at 0.5), the name at 18 (y + 2) and the
/// count right-aligned at 16 (y + 4, over the icon).
pub fn push_inventory(b: &mut UiBatch, ui: &UiFile, style: &SpriteLib, font: &Font, atlas: &str, m: &Menu, list: &ItemList, w: f32, h: f32, frame: usize) -> [f32; 3] {
    push_item_list(b, ui, style, font, atlas, m, list, w, h, frame, 3.0, true, INVENTOR_BAR)
}

/// The scroll bar's inset from the frame's right edge: inventor is always moved at open (FUN_101f1fe0 ->
/// FUN_1011a5a0: x1 - 6); a list left where it opened, or stacked on the log, keeps x1 - 8.
pub const INVENTOR_BAR: f32 = 6.0;
pub const LIST_BAR: f32 = 8.0;

/// An item list window with its rows `inset` px in ("menu    bank" rows sit at x 4) and the cursor only when focused.
/// Returns its scroll bar's (x, y, h).
#[allow(clippy::too_many_arguments)]
pub fn push_item_list(b: &mut UiBatch, ui: &UiFile, style: &SpriteLib, font: &Font, atlas: &str, m: &Menu, list: &ItemList, w: f32, h: f32, frame: usize, inset: f32, focused: bool, bar_inset: f32) -> [f32; 3] {
    let (wx, wy) = push_window(b, ui, style, m, w, h);
    let (fx, fy, fw, fh) = super::screens::frame_rect(m, w, h, 1.0, 1.0);
    let (ox, oy) = (wx + inset, wy + 4.0);
    for (k, row) in list.visible().iter().enumerate() {
        let y = oy + k as f32 * ROW_PITCH;
        // Id 0: a row without an item (the equipment set editor's rows above its items).
        if row.id != 0 {
            b.push_image(&icon_texture(row.id), ox, y, 16.0, 16.0, [1.0; 4]);
        }
        text(b, font, atlas, &row.name, ox + 18.0, y + 2.0, row.tint);
        if let Some(c) = &row.count {
            push_shape_text_right(b, ui, c.as_bytes(), ox + 16.0, y + 4.0, 0x8080_8080);
        }
    }
    if focused && !list.rows.is_empty() {
        push_cursor(b, ui, m, wx, wy, list.cursor + 1, frame);
    }
    // 10 rows (FUN_101f6700), total and top from the list every frame (FUN_101f69d0); drawn focused or not.
    super::screens::push_scroll_bar(b, ui, fx + fw - bar_inset, fy, fh, list.rows.len(), crate::inventory::VISIBLE_ROWS, list.scroll);
    [fx + fw - bar_inset, fy, fh]
}

/// The equipment grid (FUN_1012ea90): slot labels from the children, 32x32 icons at the child of
/// each slot, counts "%2d" right-aligned at (+30, +22); then the ten number fields (FUN_1012d0f0).
pub fn push_equipment(b: &mut UiBatch, ui: &UiFile, style: &SpriteLib, m: &Menu, eq: &Equipment, info: &dyn crate::inventory::ItemInfo, w: f32, h: f32, cursor: Option<usize>, frame: usize) {
    let (wx, wy) = push_window(b, ui, style, m, w, h);
    push_children(b, ui, m, wx, wy);
    for (child, id, count) in eq.icons(info) {
        let Some(e) = m.elements.get(child as usize) else { continue };
        let (x, y) = (wx + e.x as f32, wy + e.y as f32);
        b.push_image(&icon_texture(id), x, y, 32.0, 32.0, [1.0; 4]);
        if let Some(c) = count {
            push_shape_text_right(b, ui, c.trim_start().as_bytes(), x + 30.0, y + 22.0, 0x8080_8080);
        }
    }
    for (i, f) in EQUIP_FIELDS.iter().enumerate() {
        let s = crate::inventory::field_text(eq.fields[i]);
        let (x, y) = (wx + f[0] as f32, wy + f[1] as f32);
        if i >= 8 {
            push_shape_text_right(b, ui, s.as_bytes(), x + f[2] as f32, y, 0x8080_8080);
        } else {
            push_shape_text(b, ui, s.as_bytes(), x, y, 0x8080_8080);
        }
    }
    if let Some(c) = cursor {
        push_cursor(b, ui, m, wx, wy, c, frame);
    }
}

/// Sprite-font text cut to `max` px (FUN_101c04c0; the client ends a cut name with sprite 0x6c).
fn shape_clipped(b: &mut UiBatch, ui: &UiFile, s: &str, x: f32, y: f32, max: f32, tint: u32) {
    let mut cut = s.len();
    while cut > 0 && shape_text_width(ui, &s.as_bytes()[..cut]) > max {
        cut -= 1;
    }
    let end = push_shape_text(b, ui, &s.as_bytes()[..cut], x, y, tint);
    if cut < s.len()
        && let Some(lib) = ui.lib("font    fontshp")
    {
        b.push_sprite(ui, lib, 0x6c, end, y, 1.0, 1.0, tint, 0);
    }
}

/// The status panel (FUN_10141c10), offsets from the window origin; labels are in windowps #8.
pub fn push_persona(b: &mut UiBatch, ui: &UiFile, style: &SpriteLib, m: &Menu, p: &Persona, w: f32, h: f32) {
    let (x, y) = push_window(b, ui, style, m, w, h);
    let (white, red) = (0x8080_8080, 0x8080_4040);
    shape_clipped(b, ui, &p.name, x + 4.0, y + 7.0, m.elements[0].w as f32 - 6.0, white);
    if !p.ready {
        return;
    }
    let sprite = |b: &mut UiBatch, lib: &str, k: usize, sx: f32, sy: f32, tint: u32| {
        if let Some(l) = ui.lib(lib).filter(|l| k < l.sprites.len()) {
            b.push_sprite(ui, l, k, sx, sy, 1.0, 1.0, tint, 0);
        }
    };
    for (job, dy, blue) in [(&p.main, 0x13, p.main_blue), (&p.sub, 0x1f, false)] {
        if let Some((lv, j)) = job {
            let tint = if blue { 0x8020_60a0 } else { white };
            let end = push_shape_text(b, ui, lv.as_bytes(), x + 4.0, y + dy as f32, tint);
            sprite(b, "menu    windowps", 212 + *j as usize, end + 2.0, y + dy as f32, tint);
        }
    }
    let enc = |bit: u32| p.enc >> bit & 1 != 0;
    push_shape_text(b, ui, b"ILv", x + 4.0, y + 43.0, white);
    push_shape_text_right(b, ui, p.ilvl.as_bytes(), x + 70.0, y + 43.0, white);
    push_shape_text(b, ui, p.su.as_bytes(), x + 74.0, y + 43.0, white);
    for ((cur, max), dy, bit) in [(&p.hp, 0x3a, 0x17), (&p.mp, 0x46, 0x18)] {
        push_shape_text_right(b, ui, cur.as_bytes(), x + 70.0, y + dy as f32, white);
        push_shape_text(b, ui, max.as_bytes(), x + 72.0, y + dy as f32, if enc(bit) { red } else { white });
        if enc(bit) {
            sprite(b, "menu    keytops3", 430, x + 0x17 as f32, y + dy as f32 + 1.0, white);
        }
    }
    push_shape_text_right(b, ui, p.tp.as_bytes(), x + 75.0, y + 82.0, white);
    for (i, (base, bonus)) in p.stats.iter().enumerate() {
        let sy = y + 96.0 + i as f32 * 13.0;
        let on = enc(0x10 + i as u32);
        let end = push_shape_text(b, ui, base.as_bytes(), x + 50.0, sy, if on { red } else { white });
        if let Some((s, tint)) = bonus {
            push_shape_text(b, ui, s.as_bytes(), end, sy, *tint);
        }
        if on {
            sprite(b, "menu    keytops3", 430, x + 0x1f as f32, sy + 1.0, white);
        }
    }
}

/// mp_stat (FUN_10144830), what persona shows in Monstrosity: the short name clipped at (4, 7); "Lv%2d " at (4, 0x18)
/// (no job sprite); the rank {Mon, NM, HNM} and infamy right-aligned at (0x67, 0x2a) / (0x67, 0x3a); HP / MP at 0x4b /
/// 0x57 with persona's columns; TP right-aligned at (0x4b, 99); STR..CHR from 0x74, 13 apart. Contents only with
/// party record 0, the 0x119 and the Monstrosity data in.
pub fn push_mon_stat(b: &mut UiBatch, ui: &UiFile, style: &SpriteLib, m: &Menu, p: &Persona, rank: Option<&str>, infamy: &str, w: f32, h: f32) {
    let (x, y) = push_window(b, ui, style, m, w, h);
    let (white, red) = (0x8080_8080, 0x8080_4040);
    if !p.ready {
        return;
    }
    shape_clipped(b, ui, &p.name, x + 4.0, y + 7.0, m.elements[0].w as f32 - 6.0, white);
    let sprite = |b: &mut UiBatch, k: usize, sx: f32, sy: f32| {
        if let Some(l) = ui.lib("menu    keytops3").filter(|l| k < l.sprites.len()) {
            b.push_sprite(ui, l, k, sx, sy, 1.0, 1.0, white, 0);
        }
    };
    if let Some((lv, _)) = &p.main {
        push_shape_text(b, ui, lv.as_bytes(), x + 4.0, y + 0x18 as f32, if p.main_blue { 0x8020_60a0 } else { white });
    }
    if let Some(r) = rank {
        push_shape_text_right(b, ui, r.as_bytes(), x + 0x67 as f32, y + 0x2a as f32, white);
    }
    push_shape_text_right(b, ui, infamy.as_bytes(), x + 0x67 as f32, y + 0x3a as f32, white);
    let enc = |bit: u32| p.enc >> bit & 1 != 0;
    for ((cur, max), dy, bit) in [(&p.hp, 0x4b, 0x17), (&p.mp, 0x57, 0x18)] {
        if enc(bit) {
            sprite(b, 430, x + 0x17 as f32, y + dy as f32 + 1.0);
        }
        push_shape_text_right(b, ui, cur.as_bytes(), x + 0x46 as f32, y + dy as f32, white);
        push_shape_text(b, ui, max.as_bytes(), x + 0x48 as f32, y + dy as f32, if enc(bit) { red } else { white });
    }
    push_shape_text_right(b, ui, p.tp.as_bytes(), x + 0x4b as f32, y + 99.0, white);
    for (i, (base, bonus)) in p.stats.iter().enumerate() {
        let sy = y + 0x74 as f32 + i as f32 * 13.0;
        let on = enc(0x10 + i as u32);
        if on {
            sprite(b, 430, x + 0x1f as f32, sy + 1.0);
        }
        let end = push_shape_text(b, ui, base.as_bytes(), x + 0x32 as f32, sy, if on { red } else { white });
        if let Some((s, tint)) = bonus {
            push_shape_text(b, ui, s.as_bytes(), end, sy, *tint);
        }
    }
}

/// The gil window (FUN_1013b8b0): the amount right-aligned at (+0x67, +0x1f).
pub fn push_money(b: &mut UiBatch, ui: &UiFile, style: &SpriteLib, m: &Menu, gil: &str, w: f32, h: f32) {
    let (x, y) = push_window(b, ui, style, m, w, h);
    push_shape_text_right(b, ui, gil.as_bytes(), x + 103.0, y + 31.0, 0x8080_8080);
}

/// A row of the magic / ability lists (FUN_1022ce60 / FUN_10224a80): the icon sprite (library, index) at the child,
/// never greyed, the name at + 18.
pub struct ActionRow {
    pub icon: Option<(&'static str, usize)>,
    pub name: String,
    pub tint: u32,
}

/// The magic and ability lists (FUN_10221f50): rows at children 1..12, the hand at the cursor child; an empty list
/// prints its line at (10, 0x14).
/// `bar`: the whole list's (total, top) for its 12-row scroll bar (FUN_10221920), at x1 - 8 on the stacked frame;
/// returns the bar's (x, y, h).
#[allow(clippy::too_many_arguments)]
pub fn push_action_list(b: &mut UiBatch, ui: &UiFile, style: &SpriteLib, font: &Font, atlas: &str, m: &Menu, rows: &[ActionRow], cursor: usize, empty: &str, w: f32, h: f32, frame: usize, bar: (usize, usize)) -> [f32; 3] {
    let (wx, wy) = push_window(b, ui, style, m, w, h);
    let (fx, fy, fw, fh) = super::screens::frame_rect(m, w, h, 1.0, 1.0);
    if rows.is_empty() {
        for (i, line) in empty.split('\n').enumerate() {
            text(b, font, atlas, line, wx + 10.0, wy + 20.0 + 20.0 * i as f32, 0x8080_8080);
        }
        return [fx + fw - LIST_BAR, fy, fh];
    }
    for (k, r) in rows.iter().enumerate() {
        let Some(e) = m.elements.get(k + 1) else { break };
        let (x, y) = (wx + e.x as f32, wy + e.y as f32);
        if let Some((lib, k)) = r.icon
            && let Some(lib) = ui.lib(lib)
        {
            b.push_sprite(ui, lib, k, x, y, 1.0, 1.0, 0x8080_8080, 0);
        }
        text(b, font, atlas, &r.name, x + 18.0, y, r.tint);
    }
    push_cursor(b, ui, m, wx, wy, cursor + 1, frame);
    super::screens::push_scroll_bar(b, ui, fx + fw - LIST_BAR, fy, fh, bar.0, 12, bar.1);
    [fx + fw - LIST_BAR, fy, fh]
}

/// A category window (mgcmenu, abiselec ...): its children's label sprites and the hand.
#[allow(clippy::too_many_arguments)]
pub fn push_category(b: &mut UiBatch, ui: &UiFile, style: &SpriteLib, m: &Menu, cursor_child: usize, w: f32, h: f32, frame: usize) {
    let (wx, wy) = push_window(b, ui, style, m, w, h);
    push_children(b, ui, m, wx, wy);
    push_cursor(b, ui, m, wx, wy, cursor_child, frame);
}

/// subwindo (FUN_1014f960): label lines at (+5, +8) and (+5, +0x1a), their values right-aligned at +0x69.
#[allow(clippy::too_many_arguments)]
/// All in the sprite font (FUN_101c9780 / FUN_101c9b50 draw fontshp).
/// subwindo kind 9 (FUN_1014feb0): "TP" at (5, 8), "%d / " of our TP right-aligned at (0x47, 0x1a), the cost at
/// (0x69, 0x1a).
pub fn push_tp_cost_box(b: &mut UiBatch, ui: &UiFile, style: &SpriteLib, m: &Menu, tp: u32, cost: u16, w: f32, h: f32) {
    let (wx, wy) = push_window(b, ui, style, m, w, h);
    super::screens::push_shape_text(b, ui, b"TP", wx + 5.0, wy + 8.0, 0x8080_8080);
    push_shape_text_right(b, ui, format!("{tp} / ").as_bytes(), wx + 71.0, wy + 26.0, 0x8080_8080);
    push_shape_text_right(b, ui, cost.to_string().as_bytes(), wx + 105.0, wy + 26.0, 0x8080_8080);
}

pub fn push_cost_box(b: &mut UiBatch, ui: &UiFile, style: &SpriteLib, m: &Menu, lines: &[(String, String)], w: f32, h: f32) {
    let (wx, wy) = push_window(b, ui, style, m, w, h);
    for ((label, value), dy) in lines.iter().zip([8.0, 26.0]) {
        super::screens::push_shape_text(b, ui, &crate::dat::font::encode_sjis(label), wx + 5.0, wy + dy, 0x8080_8080);
        push_shape_text_right(b, ui, value.as_bytes(), wx + 105.0, wy + dy, 0x8080_8080);
    }
}
