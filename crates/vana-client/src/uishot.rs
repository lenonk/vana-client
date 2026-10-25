//! Offscreen UI renders and dumps: `ui-dump`, `title-shot`, `ui-shot`.

use crate::config::Config;
use crate::dat::{DatIndex, ui::UiFile};
use crate::render::ui::{UiBatch, render_png, write_png};
use anyhow::Result;
use std::fmt::Write as _;
use std::path::Path;

pub fn load_ui(index: &DatIndex, id: u32) -> Result<UiFile> {
    UiFile::parse(&index.read(id)?)
}

/// A UI file plus the images of the files its sprites borrow textures from.
pub fn load_ui_full(index: &DatIndex, id: u32) -> Result<UiFile> {
    let mut ui = load_ui(index, id)?;
    for &c in crate::dat::ui::companions(id) {
        ui.absorb(load_ui(index, c)?, true);
    }
    Ok(ui)
}

/// Sprite bounds in sprite space.
fn bounds(s: &crate::dat::ui::Sprite) -> Option<[f32; 4]> {
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for c in &s.cells {
        for p in c.corners {
            b[0] = b[0].min(p[0] as f32);
            b[1] = b[1].min(p[1] as f32);
            b[2] = b[2].max(p[0] as f32);
            b[3] = b[3].max(p[1] as f32);
        }
    }
    (b[0] < b[2] && b[1] < b[3]).then_some(b)
}

pub fn ui_dump(id: u32, out: &Path) -> Result<()> {
    let index = DatIndex::open(&Config::load().ffxi_dir())?;
    let mut ui = load_ui(&index, id)?;
    std::fs::create_dir_all(out)?;
    let mut list = String::new();
    let mut names: Vec<_> = ui.images.keys().collect();
    names.sort();
    for name in names {
        let img = &ui.images[name];
        writeln!(list, "image {name:?} {}x{}", img.width, img.height)?;
        write_png(&out.join(format!("img_{}.png", name.replace(' ', "_"))), img.width, img.height, &img.rgba)?;
    }
    for &c in crate::dat::ui::companions(id) {
        ui.absorb_images(load_ui(&index, c)?);
    }
    for w in &ui.warnings {
        writeln!(list, "warning {w}")?;
    }
    for m in &ui.menus {
        writeln!(list, "menu {:?} ({} elements)", m.name, m.elements.len())?;
        for (i, e) in m.elements.iter().enumerate() {
            let refs: Vec<String> = e.refs.iter().map(|r| format!("{}:{}@{}", r.kind, r.index, r.lib)).collect();
            writeln!(list, "  [{i}] id {} pos {},{} size {}x{} head {} links {:?} refs [{}] strings {:?}", e.id, e.x, e.y, e.w, e.h, hex(&e.head[6..32]), e.links, refs.join(" "), e.strings)?;
        }
    }
    // One sheet per library: sprites in a 10-wide grid of 192 px tiles, scaled down to fit.
    const TILE: f32 = 192.0;
    const COLS: usize = 10;
    const PAGE: usize = 200;
    for lib in &ui.libs {
        writeln!(list, "library {:?} textures {:?} sprites {}", lib.name, lib.textures, lib.sprites.len())?;
        for page in 0..lib.sprites.len().div_ceil(PAGE) {
        let mut batch = UiBatch::default();
        let range = page * PAGE..((page + 1) * PAGE).min(lib.sprites.len());
        for (i, s) in lib.sprites.iter().enumerate().skip(range.start).take(range.len()) {
            let k0 = i - range.start;
            let (tx, ty) = ((k0 % COLS) as f32 * TILE, (k0 / COLS) as f32 * TILE);
            batch.push_rect(tx + 1.0, ty + 1.0, TILE - 2.0, TILE - 2.0, [0.22, 0.24, 0.3, 1.0]);
            let Some(b) = bounds(s) else {
                writeln!(list, "  sprite {i}: empty")?;
                continue;
            };
            let k = ((TILE - 8.0) / (b[2] - b[0]).max(b[3] - b[1])).min(1.0);
            batch.push_sprite(&ui, lib, i, tx + 4.0 - b[0] * k, ty + 4.0 - b[1] * k, k, k, 0x8080_8080, 0);
            let cells: Vec<String> = s
                .cells
                .iter()
                .map(|c| format!("{:?}{}x{}@{},{} f{} {:?} b{:?} {}", c.corners, c.size[0], c.size[1], c.uv[0], c.uv[1], c.flip, c.colors[0], c.blend, c.texture))
                .collect();
            writeln!(list, "  sprite {i}: bounds {b:?} scale {k:.2} cells {}", cells.join(" | "))?;
        }
        let rows = range.len().div_ceil(COLS).max(1);
        let path = out.join(format!("lib_{}_{page}.png", lib.name.replace(' ', "_")));
        render_png(&[&ui], &batch, (COLS as f32 * TILE) as u32, (rows as f32 * TILE) as u32, [0.05, 0.05, 0.08], &path)?;
        }
    }
    std::fs::write(out.join("listing.txt"), list)?;
    println!("wrote {} images, {} libraries, {} menus to {}", ui.images.len(), ui.libs.len(), ui.menus.len(), out.display());
    Ok(())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join("")
}

pub fn title_shot(out: &Path, width: u32, height: u32) -> Result<()> {
    use crate::dat::ui::{LOBBY_US, WINDOW_STYLE_BASE};
    use crate::render::screens::{LOBBY_CLEAR, TitleState, title_screen};
    let index = DatIndex::open(&Config::load().ffxi_dir())?;
    let lobby = load_ui_full(&index, LOBBY_US)?;
    let style_file = load_ui(&index, WINDOW_STYLE_BASE)?;
    let style = style_file.libs.first().ok_or_else(|| anyhow::anyhow!("window style file has no sprite library"))?;
    let update = std::env::var("FFXI_TITLE_UPDATE").ok().map(|v| (v == "sel", true));
    // FFXI_TITLE_PRESS=c: Select Character pressed with counter c (9 down to 0).
    let press = std::env::var("FFXI_TITLE_PRESS").ok().and_then(|v| v.parse::<f32>().ok()).map(|c| (c, true));
    let st = TitleState { selected: 0, owned: u32::MAX, cursor_frame: 0, buttons: true, shift: 0.0, enabled: [true; 5], update, wardrobes: 0b1100, press };
    let (batch, text) = title_screen(&lobby, style, width as f32, height as f32, &st);
    for t in &text {
        println!("text slot {:?} at ({:.0}, {:.0}) align {}", t.what, t.x, t.y, t.align);
    }
    let c = LOBBY_CLEAR.map(|v| v as f64 / 255.0);
    render_png(&[&lobby], &batch, width, height, c, out)?;
    println!("wrote {}", out.display());
    Ok(())
}

pub fn ui_shot(out: &Path, width: u32, height: u32) -> Result<()> {
    use crate::dat::ui::{MENU_US, WINDOW_STYLE_BASE};
    use crate::render::screens::{HudState, hud_screen, push_party_hand};
    let index = DatIndex::open(&Config::load().ffxi_dir())?;
    let menu = load_ui_full(&index, MENU_US)?;
    let style_file = load_ui(&index, WINDOW_STYLE_BASE)?;
    let style = style_file.libs.first().ok_or_else(|| anyhow::anyhow!("window style file has no sprite library"))?;
    // FFXI_UISHOT_MEMBERS: party size (1..6, default 3).
    let n = std::env::var("FFXI_UISHOT_MEMBERS").ok().and_then(|v| v.parse().ok()).unwrap_or(3usize).clamp(1, 6);
    let party = [(1.0, 0.0), (0.55, 1.0), (0.2, 0.35), (0.9, 0.5), (0.4, 0.8), (0.7, 0.2)][..n].to_vec();
    let party_window = crate::render::screens::party_menu(party.len());
    // FFXI_UISHOT_ALLIANCE=N,M: raid1 / raid2 with N / M rows (the second out of zone, the first a leader) and the hand
    // on slot 7.
    let ally: Vec<usize> = std::env::var("FFXI_UISHOT_ALLIANCE").ok().map(|v| v.split(',').filter_map(|n| n.parse().ok()).collect()).unwrap_or_default();
    let alliance = std::array::from_fn(|k| {
        let n = ally.get(k).copied().unwrap_or(0).min(6);
        (n > 0).then(|| std::array::from_fn(|i| (i < n).then(|| crate::render::screens::AllyRow { hp: 0.3 + 0.1 * i as f32, hp_anim: 0.3 + 0.1 * i as f32, away: i == 1, marks: (i == 0) as u8 })))
    });
    let hand = (!ally.is_empty()).then_some((7, 0xa040_40d0u32, 0usize));
    let st = HudState { party, party_anim: Vec::new(), target_anim: None, target_glyph: None, party_window, party_hand: hand, alliance, party_away: Vec::new(), party_marks: vec![1], target: Some(Some(0.7)), menu_selected: Some(0), menu_page: 0, menu_off: Vec::new(), menu_swap: Vec::new(), menu_focused: true, chat_log: true };
    let (mut batch, text) = hud_screen(&menu, style, width as f32, height as f32, &st);
    push_party_hand(&mut batch, &menu, &st, width as f32, height as f32);
    for t in &text {
        println!("text slot {:?} at ({:.0}, {:.0})", t.what, t.x, t.y);
    }
    // No 3D scene here: a flat stand-in so the translucent windows read.
    render_png(&[&menu], &batch, width, height, [0.30, 0.36, 0.28], out)?;
    println!("wrote {}", out.display());
    Ok(())
}
