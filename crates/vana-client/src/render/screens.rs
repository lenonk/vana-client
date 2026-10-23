//! Whole screens laid out as FFXiMain does: the lobby title menu, and the core HUD windows.

use super::ui::UiBatch;
use crate::dat::ui::{Menu, SpriteLib, UiFile};

/// Title buttons, top to bottom, as children 1..5 of "menu    loby2win" (1-based sprite ids,
/// FFXiMain .data 0x1037beec enabled / 0x1037bef4 disabled).
pub const TITLE_BUTTONS: [(u16, u16); 5] = [(0x7f, 0x80), (0x81, 0x82), (0x83, 0x84), (0x85, 0x85), (0xb8, 0xb8)];
/// Expansion logos (0-based sprite ids, table 0x1037bf00, 11 x 20 bytes): RoZ, CoP, ToAU, WotG,
/// ACP, MKD, ASA, Visions, Scars, Heroes of Abyssea, Seekers of Adoulin.
pub const TITLE_LOGOS: [u16; 11] = [0x8d, 0x9d, 0x9f, 0xac, 0xad, 0xae, 0xaf, 0xb1, 0xb2, 0xb3, 0xb4];
/// Lobby clear colour when no zone is loaded (FUN_10183ff0(0) -> FUN_100140a0(0x80, 0x80, 0x80)).
pub const LOBBY_CLEAR: [u8; 3] = [0x80, 0x80, 0x80];

pub struct TitleState {
    /// Highlighted button 0..4 (Select, Create, Delete, Config, Back).
    pub selected: usize,
    /// Expansions owned, bit i = TITLE_LOGOS[i] (drawn dim 0x50404040 when not owned).
    pub owned: u32,
    /// Cursor hand animation frame 0..5 (advances every 3 ticks, FUN_10116c00).
    pub cursor_frame: usize,
    /// Draw the five menu buttons (off on the login screen and the lobby sub-screens).
    pub buttons: bool,
    /// Design-space x offset for the logo art and buttons; +100 centres them when no logos show.
    pub shift: f32,
    /// Per button (object +0x20 + i * 4): disabled ones use the 0x1037bef4 sprite.
    pub enabled: [bool; 5],
    /// Not retail: the Update button above Select Character, (selected, enabled); `selected` above is ignored
    /// while it is selected.
    pub update: Option<(bool, bool)>,
    /// The account's features word from the lobby (FUN_100ed7e0 case 0xf -> *0x103601bc + 0xb4): bits 2..7 are
    /// mog wardrobes 3..8 (containers 11..16).
    pub wardrobes: u32,
}

/// Wardrobe 3..8 icons on the title (FUN_10240030 after the logos): sprites 227 + i at (74 + 21 i, 360) in a
/// 640 x 480 design (FUN_10160210), scaled (1.7, 1.8) over the 1024 x 768 one; dim 0x50404040 when not available.
const WARDROBE_ICONS: usize = 227;

/// Design-space shift that centres the logo art (cell 1 of sprite 0 spans x 80..744).
pub const CENTRE_SHIFT: f32 = 100.0;

/// Where text goes that the font code draws (placeholder positions in screen pixels).
#[derive(Debug, Clone)]
pub struct TextSlot {
    pub what: &'static str,
    pub x: f32,
    pub y: f32,
    /// 0 = left aligned at x, 1 = centred on x.
    pub align: u8,
}

/// The title screen (lobby main menu). `lobby` is file 39541 with 39542 and the window style file
/// absorbed; `style` is that style's "win00" library.
pub fn title_screen(lobby: &UiFile, style: &SpriteLib, w: f32, h: f32, st: &TitleState) -> (UiBatch, Vec<TextSlot>) {
    let mut b = UiBatch::default();
    let mut text = Vec::new();
    let Some(lib) = lobby.lib("menu    lobbywin") else { return (b, text) };
    // The title draws in a 1024x768 design space stretched to the UI area (FUN_10235810, FUN_10235620).
    let (sx, sy) = (w / 1024.0, h / 768.0);

    // loby1win (FUN_1023a9c0): panel 0xba, a 2x wash of sprite 0's first cell, then sprite 0's art.
    // 0xba is the shadow panel behind the five buttons.
    if st.buttons {
        b.push_sprite(lobby, lib, 0xba, (st.shift * sx).trunc(), 0.0, sx, sy, 0xf080_8080, 0);
        // The Update button gets the same backing: the panel one pitch up, clipped to the button.
        if st.update.is_some()
            && let (Some(r), Some((_, uy, _, _)), Some((_, y1, _, _))) = (update_button_rect(lobby, w, h, st.shift), update_button_pos(lobby, w, h, st.shift), first_button_pos(lobby, w, h, st.shift))
        {
            b.set_clip(Some(r));
            b.push_sprite(lobby, lib, 0xba, (st.shift * sx).trunc(), uy - y1, sx, sy, 0xf080_8080, 0);
            b.set_clip(None);
        }
    }
    if let Some(c) = lib.sprites.first().and_then(|s| s.cells.first()) {
        b.push_cell_rect(lobby, c, -15.0, -15.0, w / 2.0 + 15.0, h / 2.0 + 15.0, 2.0, 2.0, 0x4080_8080, 0);
    }
    if let Some(s) = lib.sprites.first() {
        // The whole sprite sits at the window's position (FUN_10245120), so the copyright moves with the art.
        for c in s.cells.iter().skip(1) {
            b.push_cell(lobby, c, st.shift * sx, 0.0, sx, sy, 0xf080_8080, 0);
        }
    }

    text.extend(push_help_frame(&mut b, lobby, style, w, h));

    // loby2win (retail FUN_10240030). The year digits after "(c) 2002" are gone: the art now reads "(c) SQUARE ENIX".
    text.push(TextSlot { what: "ver.<client version>", x: w - 512.0 + 370.0, y: h - 448.0 + 364.0, align: 0 });
    if let Some(m) = lobby.menu("menu    loby2win").filter(|_| st.buttons) {
        for (i, &(on, off)) in TITLE_BUTTONS.iter().enumerate() {
            let Some(c) = m.elements.iter().find(|e| e.id as usize == i + 1) else { continue };
            let on = if st.enabled[i] { on } else { off };
            let (cx, cy) = button_pos(m, c, sx, sy, st.shift);
            let selected = i == st.selected && !st.update.is_some_and(|u| u.0);
            let tint = if selected { 0x8080_5020 } else { 0x8080_8080 };
            // The selected child of the active menu sits one pixel up-left (FUN_101155e0).
            let d = if selected { -1.0 } else { 0.0 };
            b.push_sprite(lobby, lib, on as usize - 1, cx + d, cy + d, sx, sy, tint, 0);
            if i == 0
                && let Some((upd_sel, upd_on)) = st.update
                && let (Some(spr), Some((ux, uy, _, _))) = (update_button(lib, "Update", upd_on), update_button_pos(lobby, w, h, st.shift))
            {
                let d = if upd_sel { -1.0 } else { 0.0 };
                for cell in &spr.cells {
                    b.push_cell(lobby, cell, ux + d, uy + d, sx, sy, if upd_sel { 0x8080_5020 } else { 0x8080_8080 }, 0);
                }
                if upd_sel && let Some(anc) = lobby.lib("anc     anc_s") {
                    let ox = (i16::from_le_bytes([c.head[6], c.head[7]]) + i16::from_le_bytes([m.elements[0].head[6], m.elements[0].head[7]])) as f32;
                    let oy = (i16::from_le_bytes([c.head[8], c.head[9]]) + i16::from_le_bytes([m.elements[0].head[8], m.elements[0].head[9]])) as f32;
                    b.push_sprite(lobby, anc, st.cursor_frame % anc.sprites.len().max(1), ux + ox, uy + oy, 1.0, 1.0, 0x8080_8080, 0);
                }
            }
            if selected
                && let Some(anc) = lobby.lib("anc     anc_s")
            {
                // Cursor at child pos + child/window header (+6, +8) (FUN_10116b10), unscaled.
                let ox = (i16::from_le_bytes([c.head[6], c.head[7]]) + i16::from_le_bytes([m.elements[0].head[6], m.elements[0].head[7]])) as f32;
                let oy = (i16::from_le_bytes([c.head[8], c.head[9]]) + i16::from_le_bytes([m.elements[0].head[8], m.elements[0].head[9]])) as f32;
                b.push_sprite(lobby, anc, st.cursor_frame % anc.sprites.len().max(1), cx + ox, cy + oy, 1.0, 1.0, 0x8080_8080, 0);
            }
        }
    }
    // Retail dims unowned logos (0x5040_4040); we list only what the server enables.
    for (_, &logo) in TITLE_LOGOS.iter().enumerate().filter(|(i, _)| st.owned & (1 << i) != 0) {
        b.push_sprite(lobby, lib, logo as usize, 0.0, 0.0, sx, sy, 0xff80_8080, 0);
    }
    if st.buttons {
        for i in 0..6 {
            let on = st.wardrobes >> (i + 2) & 1 != 0;
            let (x, y) = (((74 + 21 * i) as f32 * w / 640.0).trunc(), (360.0 * h / 480.0).trunc());
            let (tint, blend) = if on { (0xff80_8080, 0) } else { (0x5040_4040, 0x44) };
            b.push_sprite(lobby, lib, WARDROBE_ICONS + i, x, y, sx * 1.7, sy * 1.8, tint, blend);
        }
    }
    (b, text)
}

/// keytophd glyphs (x, y, w, h on the sheet), as retail's title buttons cut them: lower case and digits on the
/// row at y 359 (x-height letters from 364), capitals at 385.
fn keytop_glyph(c: char) -> Option<[u16; 4]> {
    let lower = "1234567890:-./@abcdefghijklmnopqrst";
    let lx = [5, 21, 37, 53, 71, 88, 104, 121, 138, 154, 172, 183, 192, 209, 223, 244, 259, 274, 288, 303, 318, 328, 344, 359, 368, 374, 388, 394, 417, 431, 447, 462, 477, 487, 499];
    let lw = [8, 13, 13, 15, 13, 13, 14, 14, 14, 15, 5, 4, 16, 13, 20, 13, 14, 13, 13, 13, 9, 14, 12, 4, 3, 13, 4, 20, 12, 14, 13, 13, 9, 11, 11];
    let i = lower.find(c)?;
    let (y, h) = match c {
        'b' | 'd' | 'f' | 'h' | 'k' | 'l' | 'i' | 'j' => (359, 20),
        't' => (361, 18),
        'g' | 'p' | 'q' => (364, 20),
        c if c.is_ascii_lowercase() => (364, 15),
        _ => (359, 20),
    };
    Some([lx[i], y, lw[i], h])
}

fn keytop_capital(c: char) -> Option<[u16; 4]> {
    let x = match c {
        'B' => 137,
        'C' => 152,
        'D' => 169,
        'S' => 405,
        'U' => 437,
        _ => return None,
    };
    let w = match c {
        'C' => 15,
        'D' => 16,
        _ => 14,
    };
    Some([x, 385, w, 20])
}

/// Not retail: an "Update" title button built as retail builds "Back" (sprite 0x85): the three b1n cells of Select
/// Character (0x7f), then the caption in keytophd at 0.7, centred, a shadow layer (alpha 49, +2, +2) under the
/// letters (alpha 192; disabled letters 65, 65, 128).
pub fn update_button(lib: &SpriteLib, caption: &str, enabled: bool) -> Option<crate::dat::ui::Sprite> {
    let base = lib.sprites.get(0x7e)?;
    let shadow = base.cells.iter().find(|c| c.texture.contains("keytophd") && c.colors[0][3] < 0x80)?.clone();
    let front = base.cells.iter().find(|c| c.texture.contains("keytophd") && c.colors[0][3] >= 0x80)?.clone();
    let mut cells: Vec<_> = base.cells.iter().filter(|c| c.texture.contains("b1n")).cloned().collect();
    let glyphs: Vec<[u16; 4]> = caption.chars().filter_map(|c| keytop_capital(c).or_else(|| keytop_glyph(c))).collect();
    let k = 0.7f32;
    let total: f32 = glyphs.iter().map(|g| (g[2] as f32 * k).round()).sum();
    let mut x = ((143.0 - total) / 2.0).round() + 1.0;
    let mut fronts = Vec::new();
    for g in &glyphs {
        let (w, h) = ((g[2] as f32 * k).round(), (g[3] as f32 * k).round());
        let top = if g[1] >= 385 { 1.0 } else { 1.0 + ((g[1] as f32 - 359.0) * k).round() };
        let quad = |dx: f32, dy: f32| {
            let (x0, y0, x1, y1) = ((x + dx) as i16, (top + dy) as i16, (x + dx + w) as i16, (top + dy + h) as i16);
            [[x0, y0], [x1, y0], [x0, y1], [x1, y1]]
        };
        let mut s = shadow.clone();
        (s.corners, s.uv, s.size) = (quad(2.0, 2.0), [g[0], g[1]], [g[2], g[3]]);
        cells.push(s);
        let mut f = front.clone();
        (f.corners, f.uv, f.size) = (quad(0.0, 0.0), [g[0], g[1]], [g[2], g[3]]);
        if !enabled {
            f.colors = [[65, 65, 128, 192]; 4];
        }
        fronts.push(f);
        x += w;
    }
    cells.extend(fronts);
    Some(crate::dat::ui::Sprite { cells })
}

fn first_button_pos(lobby: &UiFile, w: f32, h: f32, shift: f32) -> Option<(f32, f32, f32, f32)> {
    let m = lobby.menu("menu    loby2win")?;
    let c1 = m.elements.iter().find(|e| e.id == 1)?;
    let (sx, sy) = (w / 1024.0, h / 768.0);
    let (x, y) = button_pos(m, c1, sx, sy, shift);
    Some((x, y, sx, sy))
}

/// The extra button sits one button pitch above Select Character (children 1 and 2 of loby2win).
pub fn update_button_pos(lobby: &UiFile, w: f32, h: f32, shift: f32) -> Option<(f32, f32, f32, f32)> {
    let m = lobby.menu("menu    loby2win")?;
    let c1 = m.elements.iter().find(|e| e.id == 1)?;
    let c2 = m.elements.iter().find(|e| e.id == 2)?;
    let (sx, sy) = (w / 1024.0, h / 768.0);
    let (x, y) = button_pos(m, c1, sx, sy, shift);
    let pitch = ((c2.y - c1.y) as f32 * sy).trunc();
    Some((x, y - pitch, sx, sy))
}

/// Child rect = window pos + child offset, then scaled by (sx, sy) (FUN_10235620).
fn button_pos(m: &Menu, c: &crate::dat::ui::MenuElement, sx: f32, sy: f32, shift: f32) -> (f32, f32) {
    ((((m.elements[0].x + c.x) as f32 + shift) * sx).trunc(), ((m.elements[0].y + c.y) as f32 * sy).trunc())
}

/// Screen rect of the Update button, for mouse hits.
pub fn update_button_rect(lobby: &UiFile, w: f32, h: f32, shift: f32) -> Option<[f32; 4]> {
    let (x, y, sx, sy) = update_button_pos(lobby, w, h, shift)?;
    Some([x, y - 4.0 * sy, 143.0 * sx, 24.0 * sy])
}

/// Screen rects (x, y, w, h) of the five title buttons, for mouse hits.
pub fn title_button_rects(lobby: &UiFile, w: f32, h: f32, shift: f32) -> Vec<[f32; 4]> {
    let (Some(lib), Some(m)) = (lobby.lib("menu    lobbywin"), lobby.menu("menu    loby2win")) else { return Vec::new() };
    let (sx, sy) = (w / 1024.0, h / 768.0);
    let mut out = Vec::new();
    for (i, &(on, _)) in TITLE_BUTTONS.iter().enumerate() {
        let Some(c) = m.elements.iter().find(|e| e.id as usize == i + 1) else { continue };
        let Some(s) = lib.sprites.get(on as usize - 1) else { continue };
        let (cx, cy) = button_pos(m, c, sx, sy, shift);
        let pts = s.cells.iter().flat_map(|c| c.corners);
        let (x0, y0, x1, y1) = pts.fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |(a, b, c, d), p| (a.min(p[0] as f32), b.min(p[1] as f32), c.max(p[0] as f32), d.max(p[1] as f32)));
        if x0 < x1 {
            out.push([cx + x0 * sx, cy + y0 * sy, (x1 - x0) * sx, (y1 - y0) * sy]);
        }
    }
    out
}

/// lobyhelp: anchored bottom (header 0x13 = 2), frame offset (-256, 155) from its (256, 224),
/// widened to the UI width by FUN_10236ac0; the help line is centred at frame y + 5.
pub fn push_help_frame(b: &mut UiBatch, lobby: &UiFile, style: &SpriteLib, w: f32, h: f32) -> Option<TextSlot> {
    let m = lobby.menu("menu    lobyhelp")?;
    let (fx, fy, _, fh) = frame_rect(m, w, h, 1.0, 1.0);
    b.push_frame(lobby, style, fx, fy, w, fh, 1.0, 1.0, 0x8080_8080, 0, false, false);
    Some(TextSlot { what: "help line", x: w / 2.0, y: fy + 5.0, align: 1 })
}

/// The select screen's list ("menu    dbnamese", registry flags 0x1002e: unscaled, frameless): FUN_102371c0 puts
/// the 640x480 window at (W - 640, H - 480); its sprite 78 (the grid, every cell) at the origin, then every
/// child's sprite 77 (FUN_101144b0): used 0x80808080, the cursor 0x80805020 a pixel up-left; empty (state 5)
/// 0x30808080, the cursor 0x40402810 a pixel down-right. `hand`: the anc_s frame at the cursor (inferred, as the
/// other lobby menus). Returns the 16 slot rects and the grid's bounds.
pub fn select_list(b: &mut UiBatch, lobby: &UiFile, w: f32, h: f32, cursor: usize, used: &[bool], hand: Option<usize>) -> (Vec<[f32; 4]>, [f32; 4]) {
    let (Some(lib), Some(m)) = (lobby.lib("menu    lobbywin"), lobby.menu("menu    dbnamese")) else { return (Vec::new(), [0.0; 4]) };
    let (ox, oy) = (w - 640.0, h - 480.0);
    b.push_sprite(lobby, lib, 78, ox, oy, 1.0, 1.0, 0x8080_8080, 0);
    let mut rects = vec![[0.0f32; 4]; 16];
    for c in m.elements.iter().skip(1).filter(|c| (1..=16).contains(&c.id)) {
        let i = c.id as usize - 1;
        let (x, y) = (ox + c.x as f32, oy + c.y as f32);
        rects[i] = [x, y, c.w as f32, c.h as f32];
        let (tint, d) = match (used.get(i).copied().unwrap_or(false), i == cursor) {
            (true, false) => (0x8080_8080, 0.0),
            (true, true) => (0x8080_5020, -1.0),
            (false, false) => (0x3080_8080, 0.0),
            (false, true) => (0x4040_2810, 1.0),
        };
        b.push_sprite(lobby, lib, 77, x + d, y + d, 1.0, 1.0, tint, 0);
        if i == cursor
            && let (Some(frame), Some(anc)) = (hand, lobby.lib("anc     anc_s"))
        {
            b.push_sprite(lobby, anc, frame % anc.sprites.len().max(1), x, y, 1.0, 1.0, 0x8080_8080, 0);
        }
    }
    let area = lib.sprites.get(78).map_or([0.0; 4], |s| {
        let pts = s.cells.iter().flat_map(|c| c.corners);
        let (x0, y0, x1, y1) = pts.fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |(a, b, c, d), p| (a.min(p[0] as f32), b.min(p[1] as f32), c.max(p[0] as f32), d.max(p[1] as f32)));
        [ox + x0, oy + y0, x1 - x0, y1 - y0]
    });
    (rects, area)
}

/// Sprite-font text fitted into `max` px (FUN_101c9d50): the pen starts 8 in reserve; once it reaches `max` with
/// more to come (white: or a glyph wider than 8; other colours always) the rest becomes the ellipsis sprite 0x6c
/// (font table +0x1b0) and 8 px. Returns the end x.
pub fn push_shape_fit(b: &mut UiBatch, ui: &UiFile, s: &[u8], x: f32, y: f32, max: f32, tint: u32) -> f32 {
    let Some(lib) = ui.lib("font    fontshp") else { return x };
    let glyphs = shape_glyphs(s);
    let (mut pen, mut x) = (8.0, x);
    for (i, &g) in glyphs.iter().enumerate() {
        let adv = sprite_width(lib, g) - 1.0;
        pen += adv;
        if pen >= max && (tint != 0x8080_8080 || i + 1 < glyphs.len() || adv > 8.0) {
            b.push_sprite(ui, lib, 0x6c, x, y, 1.0, 1.0, tint, 0);
            return x + 8.0;
        }
        b.push_sprite(ui, lib, g, x, y, 1.0, 1.0, tint, 0);
        x += adv;
    }
    x
}

/// FUN_101c96d0: how many leading bytes of `s` fit in `max` px (each glyph advances its width - 1).
fn shape_fit_count(lib: &SpriteLib, s: &[u8], max: f32) -> usize {
    let mut pen = 0.0;
    for (i, &c) in s.iter().enumerate() {
        let adv = shape_glyphs(&[c]).first().map_or(0.0, |&g| sprite_width(lib, g) - 1.0);
        if pen + adv > max {
            return i;
        }
        pen += adv;
    }
    s.len()
}

/// FUN_101c9fa0: a name in up to two (three) lines of `max` px at (x, y), as the target window draws it. Cut to 24
/// bytes; one line (at y + 6 when `centre`) when it fits and has no ':'; otherwise broken after the first ':' before
/// the fit, else at the last ' ', '-', '@', ''' or ':' from fit + 1 down to fit / 4, else at the fit. `split` (no
/// gauge): the rest goes on as its own name one line down, 4 in and 4 narrower; else the rest is fitted with the
/// ellipsis (FUN_101c9d50).
#[allow(clippy::too_many_arguments)]
pub fn push_wrapped_name(b: &mut UiBatch, ui: &UiFile, s: &[u8], max: f32, x: f32, y: f32, tint: u32, centre: bool, split: bool) {
    let Some(lib) = ui.lib("font    fontshp") else { return };
    let s = &s[..s.len().min(24)];
    let fit = shape_fit_count(lib, s, max);
    let colon = s.iter().position(|&c| c == b':');
    if s.len() <= fit && colon.is_none() {
        push_shape_text(b, ui, s, x, if centre { y + 6.0 } else { y }, tint);
        return;
    }
    let mut cut = fit;
    match colon {
        Some(i) if i < fit && i + 1 != fit => cut = i + 1,
        _ => {
            let mut i = fit + 1;
            while i >= fit / 4 && i > 0 {
                if s.get(i).is_some_and(|c| matches!(c, b' ' | b'-' | b'@' | b'\'' | b':')) {
                    cut = i + 1;
                    break;
                }
                i -= 1;
            }
        }
    }
    let cut = cut.min(s.len());
    if split {
        push_shape_text(b, ui, &s[..cut], x, y, tint);
        push_wrapped_name(b, ui, &s[cut..], max - 4.0, x + 4.0, y + 12.0, tint, false, false);
        return;
    }
    let rest_fits = shape_fit_count(lib, &s[cut..], max - 4.0) >= s.len() - cut;
    let cut = if rest_fits { cut } else { fit.min(s.len()) };
    push_shape_text(b, ui, &s[..cut], x, y, tint);
    push_shape_fit(b, ui, &s[cut..], x + 4.0, y + 12.0, max - 4.0, tint);
}

/// Sprite width as the client advances text by it (+0x22 - +0x20: bounds in x).
pub fn sprite_width(lib: &SpriteLib, i: usize) -> f32 {
    let Some(s) = lib.sprites.get(i) else { return 0.0 };
    let xs = s.cells.iter().flat_map(|c| c.corners.iter().map(|p| p[0]));
    let (lo, hi) = xs.fold((i16::MAX, i16::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
    if lo > hi { 0.0 } else { (hi - lo) as f32 }
}

/// Window placement (FUN_101160d0): file pos (scaled by (fx, fy) for SCALED menus), shifted by
/// W - 512 / H - 448 when header byte 0x13 anchors right (1), bottom (2) or both (3).
pub fn window_pos(m: &Menu, w: f32, h: f32, fx: f32, fy: f32) -> (f32, f32) {
    let e = &m.elements[0];
    let (mut x, mut y) = ((e.x as f32 * fx).trunc(), (e.y as f32 * fy).trunc());
    let anchor = e.head[0x13];
    if anchor & 1 != 0 {
        x += w - 512.0;
    }
    if anchor & 2 != 0 {
        y += h - 448.0;
    }
    (x, y)
}

/// The frame rectangle: window pos + header (+0x0e, +0x10), size from the header (FUN_101144b0).
pub fn frame_rect(m: &Menu, w: f32, h: f32, fx: f32, fy: f32) -> (f32, f32, f32, f32) {
    let e = &m.elements[0];
    let (x, y) = window_pos(m, w, h, fx, fy);
    let ox = i16::from_le_bytes([e.head[0x0e], e.head[0x0f]]) as f32;
    let oy = i16::from_le_bytes([e.head[0x10], e.head[0x11]]) as f32;
    (x + ox * fx, y + oy * fy, e.w as f32 * fx, e.h as f32 * fy)
}

/// Draws a menu window as FUN_101144b0 does for an unscaled menu: the window-style frame over the
/// frame rect, then the window's own sprite (its first cell, the background, is skipped).
/// Returns the window position.
pub fn push_window(b: &mut UiBatch, ui: &UiFile, style: &SpriteLib, m: &Menu, w: f32, h: f32) -> (f32, f32) {
    push_window_at(b, ui, style, m, w, h, 0.0)
}

/// `push_window` moved down by `dy` (region stacking).
pub fn push_window_at(b: &mut UiBatch, ui: &UiFile, style: &SpriteLib, m: &Menu, w: f32, h: f32, dy: f32) -> (f32, f32) {
    push_window_alpha(b, ui, style, m, w, h, dy, 1.0)
}

/// `push_window_at` at frame alpha `a` (part +0x0c): colour ftol(a * 128) << 24 | 0x808080, the frame alpha
/// blended (0x44) while a < 1 (FUN_101144b0 at 0x101147a8).
#[allow(clippy::too_many_arguments)]
pub fn push_window_alpha(b: &mut UiBatch, ui: &UiFile, style: &SpriteLib, m: &Menu, w: f32, h: f32, dy: f32, a: f32) -> (f32, f32) {
    let (x, y) = window_pos(m, w, h, 1.0, 1.0);
    let (fx, fy, fw, fh) = frame_rect(m, w, h, 1.0, 1.0);
    let (y, fy) = (y + dy, fy + dy);
    let tint = alpha_tint(a);
    b.push_frame(ui, style, fx, fy, fw, fh, 1.0, 1.0, tint, if a < 1.0 { 0x44 } else { 0 }, false, false);
    if let Some(r) = m.elements[0].refs.iter().find(|r| r.kind == 0)
        && let Some(lib) = ui.lib(&r.lib)
    {
        b.push_sprite_from(ui, lib, r.index as usize, 1, x, y, 1.0, 1.0, tint, 0);
    }
    (x, y)
}

/// A window part's colour at alpha factor `a`.
pub fn alpha_tint(a: f32) -> u32 {
    ((a.clamp(0.0, 1.0) * 128.0) as u32) << 24 | 0x80_8080
}

/// A gauge (FUN_101c1190): fill sprite at (+4, +1) scaled in x by the fraction, blending off;
/// then the frame sprite at (x, y), dimmed 0x30505050 when empty.
pub fn push_gauge(b: &mut UiBatch, ui: &UiFile, lib: &SpriteLib, x: f32, y: f32, frame: usize, fill: usize, fraction: f32) {
    let f = fraction.clamp(0.0, 1.0);
    if f > 0.0 {
        b.push_sprite(ui, lib, fill, x + 4.0, y + 1.0, f, 1.0, 0x8080_8080, 8);
    }
    b.push_sprite(ui, lib, frame, x, y, 1.0, 1.0, if f == 0.0 { 0x3050_5050 } else { 0x8080_8080 }, 0);
}

pub struct HudState {
    /// HP / MP fraction per party member (first is the player).
    pub party: Vec<(f32, f32)>,
    /// The target window: Some(HP fraction) with a gauge, Some(None) without one.
    pub target: Option<Option<f32>>,
    /// Highlighted main menu item (child index from 0), None = menu closed.
    pub menu_selected: Option<usize>,
    /// Main menu page: 0 commands (menuwind), 1 social (socialme); the arrows swap them.
    pub menu_page: u8,
    /// Draw logwindo at its layout size (offscreen tools); the client sizes it itself (client/logwin.rs).
    pub chat_log: bool,
}

/// The main menu window of a page (FUN_1013b3f0 / FUN_10147fc0 open each other on left / right).
pub fn menu_window(page: u8) -> &'static str {
    if page == 0 { "menu    menuwind" } else { "menu    socialme" }
}

/// Glyph sprite of "font    fontshp" for a byte (FUN_101bff20): byte - 0x20, with 0x85 0xDA / 0x85 0xC8
/// pairs mapping to sprites 95 / 96 and a lone 0x85 skipped.
fn shape_glyphs(s: &[u8]) -> Vec<usize> {
    let mut out = Vec::with_capacity(s.len());
    for (i, &c) in s.iter().enumerate() {
        let prev = if i > 0 { s[i - 1] } else { 0 };
        let g = match (prev, c) {
            (_, 0x85) => continue,
            (0x85, 0xDA) => 95,
            (0x85, 0xC8) => 96,
            _ if (0x20..0x20 + 0x8c).contains(&c) => (c - 0x20) as usize,
            _ => continue,
        };
        out.push(g);
    }
    out
}

/// Sprite-font text left to right from x (FUN_101bfef0): each glyph advances its width - 1. Returns the end x.
pub fn push_shape_text(b: &mut UiBatch, ui: &UiFile, s: &[u8], x: f32, y: f32, tint: u32) -> f32 {
    let Some(lib) = ui.lib("font    fontshp") else { return x };
    let mut x = x;
    for g in shape_glyphs(s) {
        b.push_sprite(ui, lib, g, x, y, 1.0, 1.0, tint, 0);
        x += sprite_width(lib, g) - 1.0;
    }
    x
}

/// Sprite-font text ending at x, drawn from its last glyph back (FUN_101c02c0, used for numbers).
pub fn push_shape_text_right(b: &mut UiBatch, ui: &UiFile, s: &[u8], x: f32, y: f32, tint: u32) -> f32 {
    let Some(lib) = ui.lib("font    fontshp") else { return x };
    let mut x = x;
    for g in shape_glyphs(s).into_iter().rev() {
        x -= sprite_width(lib, g) - 1.0;
        b.push_sprite(ui, lib, g, x, y, 1.0, 1.0, tint, 0);
    }
    x
}

/// Party names (FUN_101c08f0): longer than 8 bytes become the first 7 and the ellipsis sprite 0x6c.
pub fn push_shape_name(b: &mut UiBatch, ui: &UiFile, name: &[u8], x: f32, y: f32, tint: u32) -> f32 {
    if name.len() <= 8 {
        return push_shape_text(b, ui, name, x, y, tint);
    }
    let x = push_shape_text(b, ui, &name[..7], x, y, tint);
    let Some(lib) = ui.lib("font    fontshp") else { return x };
    b.push_sprite(ui, lib, 0x6c, x, y, 1.0, 1.0, tint, 0);
    x + sprite_width(lib, 0x6c) - 1.0
}

/// Width of sprite-font text.
pub fn shape_text_width(ui: &UiFile, s: &[u8]) -> f32 {
    let Some(lib) = ui.lib("font    fontshp") else { return 0.0 };
    shape_glyphs(s).into_iter().map(|g| sprite_width(lib, g) - 1.0).sum()
}

/// First cell colour of a sprite as an ARGB tint (FUN_1011c410), as loaded (dat::ui::cell_color: 0x7f is 0x80 = 1.0).
pub fn sprite_tint(ui: &UiFile, lib: &str, k: usize) -> u32 {
    let Some(c) = ui.lib(lib).and_then(|l| l.sprites.get(k)).and_then(|s| s.cells.first()) else { return 0x8080_8080 };
    let [r, g, b, a] = c.colors[0];
    u32::from_be_bytes([a, r, g, b])
}

/// A name plate (FUN_10084b20): fontshp glyphs scaled by (sx, sy), each advancing its full width,
/// centred on x and hanging down from y (the letters' top edge, min y 0, sits on the anchor), alpha blended
/// (the pass sets SRCALPHA / INVSRCALPHA).
pub fn push_name_plate(b: &mut UiBatch, ui: &UiFile, s: &[u8], x: f32, y: f32, sx: f32, sy: f32, tint: u32) {
    let Some(lib) = ui.lib("font    fontshp") else { return };
    let glyphs = shape_glyphs(s);
    let total: f32 = glyphs.iter().map(|&g| sprite_width(lib, g)).sum();
    let (mut px, py) = ((x - total * sx * 0.5).round(), y.round());
    for g in glyphs {
        b.push_sprite(ui, lib, g, px, py, sx, sy, tint, 0x44);
        px += sprite_width(lib, g) * sx;
    }
}


/// HP number colour (FUN_101f93e0): "menu    hpcol" sprite 3 below 25 %, 2 below 50, 1 below 75, else 0;
/// its first cell's colour as an ARGB tint.
pub fn hp_tint(ui: &UiFile, hpp: u8) -> u32 {
    let k = match hpp {
        0..25 => 3,
        25..50 => 2,
        50..75 => 1,
        _ => 0,
    };
    sprite_tint(ui, "menu    hpcol", k)
}

/// Rects of the main menu's left / right arrows (children 13 and 14).
pub fn hud_menu_arrows(ui: &UiFile, w: f32, h: f32, page: u8) -> Vec<[f32; 4]> {
    let Some(m) = ui.menu(menu_window(page)) else { return Vec::new() };
    let (x, y) = window_pos(m, w, h, 1.0, 1.0);
    m.elements.iter().filter(|c| c.id == 13 || c.id == 14).map(|c| [x + c.x as f32 - 4.0, y + c.y as f32 - 4.0, c.w as f32 + 8.0, c.h as f32 + 8.0]).collect()
}

/// A 512x448-space menu window (FUN_101144b0): frame, window sprite, each child's kind-0 sprite (labels,
/// arrows), the selected label tinted and nudged with the hand cursor. `sel` counts the windowps labels
/// in file order; returns their rects.
pub fn push_menu(b: &mut UiBatch, ui: &UiFile, style: &SpriteLib, name: &str, w: f32, h: f32, sel: usize) -> Vec<[f32; 4]> {
    let Some(m) = ui.menu(name) else { return Vec::new() };
    let (x, y) = push_window(b, ui, style, m, w, h);
    let mut rects = Vec::new();
    for c in m.elements.iter().skip(1) {
        let Some(r) = c.refs.iter().find(|r| r.kind == 0) else { continue };
        let Some(lib) = ui.lib(&r.lib) else { continue };
        let (cx, cy) = (x + c.x as f32, y + c.y as f32);
        let label = r.lib.ends_with("windowps");
        let on = label && rects.len() == sel;
        let d = if on { -1.0 } else { 0.0 };
        b.push_sprite(ui, lib, r.index as usize, cx + d, cy + d, 1.0, 1.0, if on { 0x8080_5020 } else { 0x8080_8080 }, 0);
        if on && let Some(anc) = ui.lib("anc     anc_s") {
            b.push_sprite(ui, anc, 0, cx, cy, 1.0, 1.0, 0x8080_8080, 0);
        }
        if label {
            rects.push([cx, cy, c.w as f32, c.h as f32]);
        }
    }
    rects
}

/// A menu window with each child's kind-0 sprite at the child's place, child `sel` tinted 0x80805020 and
/// nudged up-left with the anc_s hand (FUN_101144b0). Returns the window position.
pub fn push_menu_sel(b: &mut UiBatch, ui: &UiFile, style: &SpriteLib, name: &str, w: f32, h: f32, sel: u16) -> Option<(f32, f32)> {
    let m = ui.menu(name)?;
    let (x, y) = push_window(b, ui, style, m, w, h);
    for c in m.elements.iter().skip(1) {
        let Some(r) = c.refs.iter().find(|r| r.kind == 0) else { continue };
        let Some(lib) = ui.lib(&r.lib) else { continue };
        let (cx, cy) = (x + c.x as f32, y + c.y as f32);
        let on = c.id == sel;
        let d = if on { -1.0 } else { 0.0 };
        b.push_sprite(ui, lib, r.index as usize, cx + d, cy + d, 1.0, 1.0, if on { 0x8080_5020 } else { 0x8080_8080 }, 0);
        if on && let Some(anc) = ui.lib("anc     anc_s") {
            b.push_sprite(ui, anc, 0, cx, cy, 1.0, 1.0, 0x8080_8080, 0);
        }
    }
    Some((x, y))
}

/// A menu window's frame rect on screen (x, y, w, h).
pub fn window_rect(ui: &UiFile, name: &str, w: f32, h: f32) -> Option<[f32; 4]> {
    let (x, y, fw, fh) = frame_rect(ui.menu(name)?, w, h, 1.0, 1.0);
    Some([x, y, fw, fh])
}

/// Region stacking (FUN_10159080 -> FUN_10158e10): windows with registry flag 0x2000 (targetwi, flags
/// 0x02002401) sit on region 1's window, the party window: frame top = party frame top - frame height - 2,
/// at least 16. Returns the y offset from the window's layout place (0 for other windows).
pub fn stacked_dy(ui: &UiFile, name: &str, w: f32, h: f32, members: usize) -> f32 {
    if name != "menu    targetwi" {
        return 0.0;
    }
    let (Some(t), Some(p)) = (ui.menu(name), ui.menu(&party_menu(members))) else { return 0.0 };
    let (_, ty, _, th) = frame_rect(t, w, h, 1.0, 1.0);
    let (_, py, _, _) = frame_rect(p, w, h, 1.0, 1.0);
    (py - th - 2.0).max(16.0) - ty
}

/// Main menu items (children 1..12) top to bottom: (index for HudState::menu_selected, rect).
pub fn hud_menu_items(ui: &UiFile, w: f32, h: f32, page: u8) -> Vec<(usize, [f32; 4])> {
    let Some(m) = ui.menu(menu_window(page)) else { return Vec::new() };
    let (x, y) = window_pos(m, w, h, 1.0, 1.0);
    let mut items: Vec<(usize, [f32; 4])> = m
        .elements
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, c)| (1..=12).contains(&c.id))
        .map(|(i, c)| (i - 1, [x + c.x as f32, y + c.y as f32, c.w as f32, c.h as f32]))
        .collect();
    items.sort_by(|a, b| a.1[1].total_cmp(&b.1[1]));
    items
}

/// The party window swaps layouts by member count (FUN_101facc0): "ptw0" outside a party, else
/// "ptw1".."ptw6" from the table at 0x10378d30 + n * 0x20. We are never in a party yet.
pub fn party_menu(members: usize) -> String {
    match members {
        0 | 1 => "menu    ptw0".into(),
        n => format!("menu    ptw{}", n.min(6)),
    }
}

/// windowps gauge sprites (FUN_101c1520 / FUN_101c1640 via the windowps array).
pub const GAUGE_HP: (usize, usize) = (38, 97);
pub const GAUGE_MP: (usize, usize) = (39, 98);

/// The core HUD: chat log, party, target and the main menu, in FFXiMain's 512x448 layout
/// anchored to the UI area's edges (no scaling).
pub fn hud_screen(ui: &UiFile, style: &SpriteLib, w: f32, h: f32, st: &HudState) -> (UiBatch, Vec<TextSlot>) {
    let mut b = UiBatch::default();
    let mut text = Vec::new();
    let Some(wps) = ui.lib("menu    windowps") else { return (b, text) };
    if let Some(m) = ui.menu("menu    logwindo").filter(|_| st.chat_log) {
        let (x, y) = push_window(&mut b, ui, style, m, w, h);
        text.push(TextSlot { what: "chat log lines", x: x + 8.0, y: y + 6.0, align: 0 });
    }
    if let Some(m) = ui.menu("menu    targetwi")
        && let Some(hp) = st.target
    {
        let dy = stacked_dy(ui, "menu    targetwi", w, h, st.party.len());
        let (x, y) = push_window_at(&mut b, ui, style, m, w, h, dy);
        // FUN_10149900: name at (+5, +6), HP gauge at (+0x1c, +0x1d) unless the actor hides it.
        text.push(TextSlot { what: "target name", x: x + 5.0, y: y + 6.0, align: 0 });
        if let Some(hp) = hp {
            push_gauge(&mut b, ui, wps, x + 28.0, y + 29.0, GAUGE_HP.0, GAUGE_HP.1, hp);
        }
    }
    if let Some(m) = ui.menu(&party_menu(st.party.len())) {
        let (x, y) = push_window(&mut b, ui, style, m, w, h);
        // FUN_101f9560 per member row (children 1..6): HP gauge (+26, +7), MP gauge (+49, +17, drawn empty
        // for jobs without MP), name at the row, HP number (+88, +1), MP number (+98, +10); TP (+48, +10) only
        // with config 0xae bit 0 (FUN_1018c830(0xae, 0), default 0).
        for (i, &(hp, mp)) in st.party.iter().take(6).enumerate() {
            let Some(c) = m.elements.iter().find(|e| e.id as usize == i + 1) else { continue };
            let (rx, ry) = (x + c.x as f32, y + c.y as f32);
            push_gauge(&mut b, ui, wps, rx + 26.0, ry + 7.0, GAUGE_HP.0, GAUGE_HP.1, hp);
            push_gauge(&mut b, ui, wps, rx + 49.0, ry + 17.0, GAUGE_MP.0, GAUGE_MP.1, mp);
            text.push(TextSlot { what: "member name", x: rx, y: ry, align: 0 });
            text.push(TextSlot { what: "HP", x: rx + 88.0, y: ry + 1.0, align: 0 });
            text.push(TextSlot { what: "MP", x: rx + 98.0, y: ry + 10.0, align: 0 });
        }
    }
    if let Some(m) = ui.menu(menu_window(st.menu_page))
        && let Some(sel) = st.menu_selected
    {
        let (x, y) = push_window(&mut b, ui, style, m, w, h);
        // Items are label sprites at their child positions; the selected one is tinted and nudged
        // up-left, with the hand cursor (anc_s) at its position (FUN_101144b0, FUN_10116b10).
        for (i, c) in m.elements.iter().skip(1).enumerate() {
            let Some(r) = c.refs.iter().find(|r| r.kind == 0) else { continue };
            let Some(lib) = ui.lib(&r.lib) else { continue };
            let (cx, cy) = (x + c.x as f32, y + c.y as f32);
            let on = i == sel;
            let d = if on { -1.0 } else { 0.0 };
            b.push_sprite(ui, lib, r.index as usize, cx + d, cy + d, 1.0, 1.0, if on { 0x8080_5020 } else { 0x8080_8080 }, 0);
            if on && let Some(anc) = ui.lib("anc     anc_s") {
                b.push_sprite(ui, anc, 0, cx, cy, 1.0, 1.0, 0x8080_8080, 0);
            }
        }
    }
    (b, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dat::ui::{LOBBY_US, UiFile, companions};

    #[test]
    fn shape_space_width() {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let Ok(file) = index.read(LOBBY_US) else { return };
        let mut ui = UiFile::parse(&file).unwrap();
        for &c in companions(LOBBY_US) {
            ui.absorb(UiFile::parse(&index.read(c).unwrap()).unwrap(), true);
        }
        // Space is 4 wide, so HUD text (width - 1 per glyph) leaves a 3 px gap; letters are 9.
        let lib = ui.lib("font    fontshp").unwrap();
        assert_eq!([0usize, 0x34, 0x45].map(|g| sprite_width(lib, g)), [4.0, 9.0, 9.0]);
        assert_eq!(shape_text_width(&ui, b"a b") - shape_text_width(&ui, b"ab"), 3.0);
    }
}
