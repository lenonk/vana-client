//! Menu resources: IMG textures (0x20), sprite libraries (0x31) and menu layouts (0x30).
//!
//! PC US file ids: 39542 in-game menus, 39541 lobby/title (78/79 are the PS2-era US set).

use super::{chunks, zone::parse_img};
use anyhow::{Result, bail};
use std::collections::HashMap;

pub const MENU_US: u32 = 39542;
pub const LOBBY_US: u32 = 39541;
/// Window frame styles 1..8 (config "window type"): FFXiMain loads file 14 + style (FUN_10159a70).
pub const WINDOW_STYLE_BASE: u32 = 14;
/// Icon images ("menu    magiconw", lobycwin's "menu    ustats"), resident from startup (FUN_10010080 loads 0x9a7f).
pub const ICONS: u32 = 39551;

/// Files whose images a UI file's sprites also draw from.
pub fn companions(id: u32) -> &'static [u32] {
    match id {
        LOBBY_US => &[MENU_US, WINDOW_STYLE_BASE, ICONS],
        MENU_US => &[WINDOW_STYLE_BASE],
        _ => &[],
    }
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn i16_at(b: &[u8], o: usize) -> i16 {
    u16_at(b, o) as i16
}

/// US library a menu's sprite ref names (FUN_101185f0 via FUN_10250e90 rows 0, 1, 5; table 0x1037e1f0).
pub fn lang_lib(name: &str) -> Option<&'static str> {
    match name {
        "menu    frames" => Some("menu    framesus"),
        "menu    key1top" => Some("menu    key1topu"),
        // Layouts name the PS2 library; FUN_10118090 renames lobbyps2 to lobbywin on load.
        "menu    lobbyps2" => Some("menu    lobbywin"),
        _ => None,
    }
}

/// A cell corner colour as FUN_1011adf0 loads it: RGB + 1 below 255, alpha 1..254 -> min((a + 1) * 1.5, 255).
pub fn cell_color([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    let up = |v: u8| if v < 0xff { v + 1 } else { v };
    let a = if a == 0 || a == 0xff { a } else { ((a as f32 + 1.0) * 1.5).min(255.0) as u8 };
    [up(r), up(g), up(b), a]
}

/// 16-byte resource names ("menu    yubi    "): compared case-insensitively, trailing blanks dropped.
pub fn norm_name(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim_end().to_ascii_lowercase()
}

pub struct UiImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// One textured quad of a sprite (61 bytes on disk).
#[derive(Clone, Debug)]
pub struct Cell {
    /// Corners TL, TR, BL, BR in sprite space (pixels).
    pub corners: [[i16; 2]; 4],
    pub size: [u16; 2],
    pub uv: [u16; 2],
    /// Bit 0 mirrors U, bit 1 mirrors V.
    pub flip: u8,
    /// RGBA per corner, PS2 scale (0x80 = 1.0).
    pub colors: [[u8; 4]; 4],
    /// PS2 GS alpha blend: enable, A, B, D of (A - B) * As + D with 0 = source, 1 = frame, 2 = zero.
    pub blend: [u8; 4],
    pub texture: String,
}

pub const CELL_SIZE: usize = 61;

#[derive(Clone, Debug, Default)]
pub struct Sprite {
    pub cells: Vec<Cell>,
}

#[derive(Clone)]
pub struct SpriteLib {
    pub name: String,
    pub textures: Vec<String>,
    pub sprites: Vec<Sprite>,
}

pub fn parse_sprite_lib(p: &[u8]) -> Result<SpriteLib> {
    if p.len() < 19 {
        bail!("sprite library too short");
    }
    let name = norm_name(&p[0..16]);
    let ntex = p[16] as usize;
    let mut at = 17;
    let mut textures = Vec::with_capacity(ntex);
    for _ in 0..ntex {
        textures.push(norm_name(p.get(at..at + 16).ok_or_else(|| anyhow::anyhow!("{name}: texture list cut"))?));
        at += 16;
    }
    let nsprites = u16_at(p, at) as usize;
    at += 2;
    let mut sprites = Vec::with_capacity(nsprites);
    for i in 0..nsprites {
        let Some(&ncells) = p.get(at) else { bail!("{name}: sprite {i} of {nsprites} cut") };
        at += 1;
        let mut cells = Vec::with_capacity(ncells as usize);
        for _ in 0..ncells {
            let c = p.get(at..at + CELL_SIZE).ok_or_else(|| anyhow::anyhow!("{name}: cell cut in sprite {i}"))?;
            let corner = |k: usize| [i16_at(c, k * 4), i16_at(c, k * 4 + 2)];
            let color = |k: usize| cell_color([c[0x19 + k * 4], c[0x1A + k * 4], c[0x1B + k * 4], c[0x1C + k * 4]]);
            cells.push(Cell {
                corners: [corner(0), corner(1), corner(2), corner(3)],
                size: [u16_at(c, 0x10), u16_at(c, 0x12)],
                uv: [u16_at(c, 0x14), u16_at(c, 0x16)],
                flip: c[0x18],
                colors: [color(0), color(1), color(2), color(3)],
                blend: [c[0x29], c[0x2A], c[0x2B], c[0x2C]],
                texture: norm_name(&c[0x2D..0x3D]),
            });
            at += CELL_SIZE;
        }
        sprites.push(Sprite { cells });
    }
    Ok(SpriteLib { name, textures, sprites })
}

/// A sprite reference of a menu element: (state kind, sprite index, library).
#[derive(Clone, Debug)]
pub struct SpriteRef {
    pub kind: u16,
    pub index: u16,
    pub lib: String,
}

/// One element of a menu layout: the window (first) or a child (cursor stop).
#[derive(Clone, Debug)]
pub struct MenuElement {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
    /// The 32-byte header as stored (fields not decoded yet).
    pub head: [u8; 32],
    /// Child id and its cursor links (children only).
    pub id: u16,
    pub links: [u8; 6],
    pub refs: Vec<SpriteRef>,
    pub strings: [String; 2],
}

#[derive(Clone)]
pub struct Menu {
    pub name: String,
    pub elements: Vec<MenuElement>,
}

/// Window header keeps its counts at 0x14..0x16, child headers at 0x1B..0x1E (FUN_10118090 / FUN_10118390).
fn parse_element(p: &[u8], at: usize, child: bool) -> Result<(MenuElement, usize)> {
    let e = p.get(at..at + 32).ok_or_else(|| anyhow::anyhow!("element header cut"))?;
    let size = u16_at(e, 0) as usize;
    let (n, l1, l2) = if child { (e[0x1B], e[0x1D], e[0x1E]) } else { (e[0x14], e[0x15], e[0x16]) };
    let mut o = at + 32;
    let mut refs = Vec::new();
    for _ in 0..n {
        let r = p.get(o..o + 20).ok_or_else(|| anyhow::anyhow!("sprite ref cut"))?;
        refs.push(SpriteRef { kind: u16_at(r, 0), index: u16_at(r, 2), lib: norm_name(&r[4..20]) });
        o += 20;
    }
    let s1 = norm_name(p.get(o..o + l1 as usize).unwrap_or(&[]));
    o += l1 as usize + 1;
    let s2 = norm_name(p.get(o..o + l2 as usize).unwrap_or(&[]));
    let mut links = [0u8; 6];
    if child {
        links.copy_from_slice(&e[0x15..0x1B]);
    }
    let el = MenuElement {
        x: i16_at(e, 2),
        y: i16_at(e, 4),
        w: u16_at(e, 10),
        h: u16_at(e, 12),
        head: e.try_into().unwrap(),
        id: if child { u16_at(e, 0x12) } else { 0 },
        links,
        refs,
        strings: [s1, s2],
    };
    if size < 32 {
        bail!("element size {size}");
    }
    Ok((el, at + size))
}

pub fn parse_menu(p: &[u8]) -> Result<Menu> {
    if p.len() < 32 {
        bail!("menu too short");
    }
    let name = norm_name(&p[0..16]);
    let children = p[0x11] as usize;
    let (win, mut at) = parse_element(p, 0x20, false)?;
    let mut elements = vec![win];
    for _ in 0..children {
        let (el, next) = parse_element(p, at, true)?;
        elements.push(el);
        at = next;
    }
    Ok(Menu { name, elements })
}

/// Everything UI-related in one DAT.
#[derive(Default)]
pub struct UiFile {
    pub images: HashMap<String, UiImage>,
    pub libs: Vec<SpriteLib>,
    pub menus: Vec<Menu>,
    pub warnings: Vec<String>,
}

impl UiFile {
    pub fn parse(file: &[u8]) -> Result<Self> {
        let mut out = UiFile::default();
        for c in chunks(file)? {
            match c.kind {
                0x20 => match parse_img(c.data) {
                    Ok(t) => {
                        let key = norm_name(&c.data[1..17]);
                        out.images.insert(key, UiImage { width: t.width, height: t.height, rgba: t.rgba });
                    }
                    Err(e) => out.warnings.push(format!("{}: {e}", c.name())),
                },
                0x31 => match parse_sprite_lib(c.data) {
                    Ok(l) => out.libs.push(l),
                    Err(e) => out.warnings.push(format!("{}: {e}", c.name())),
                },
                0x30 => match parse_menu(c.data) {
                    Ok(m) => out.menus.push(m),
                    Err(e) => out.warnings.push(format!("{}: {e}", c.name())),
                },
                _ => {}
            }
        }
        Ok(out)
    }

    /// Texture names are global in the client: take another file's images where ours have none.
    pub fn absorb_images(&mut self, other: UiFile) {
        self.absorb(other, false);
    }

    /// Sprite libraries (and menus) are looked up by name across loaded files as well (FUN_1011cd60).
    pub fn absorb(&mut self, other: UiFile, libs: bool) {
        for (k, v) in other.images {
            self.images.entry(k).or_insert(v);
        }
        if libs {
            for l in other.libs {
                if self.lib(&l.name).is_none() {
                    self.libs.push(l);
                }
            }
            for m in other.menus {
                if self.menu(&m.name).is_none() {
                    self.menus.push(m);
                }
            }
        }
    }

    pub fn lib(&self, name: &str) -> Option<&SpriteLib> {
        let name = name.to_ascii_lowercase();
        let name = lang_lib(&name).unwrap_or(&name);
        self.libs.iter().find(|l| l.name == name)
    }

    pub fn menu(&self, name: &str) -> Option<&Menu> {
        let name = name.to_ascii_lowercase();
        self.menus.iter().find(|m| m.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_colour_as_ffximain_loads_it() {
        assert_eq!(cell_color([0x7f, 0x40, 0, 0x7f]), [0x80, 0x41, 1, 192]);
        assert_eq!(cell_color([0xff, 0xfe, 0x7f, 0x20]), [0xff, 0xff, 0x80, 49]);
        assert_eq!([0, 8, 0x40, 0x70, 0xa9, 0xff].map(|a| cell_color([0, 0, 0, a])[3]), [0, 13, 97, 169, 255, 255]);
    }

    /// Needs the installed game; skipped without it.
    #[test]
    fn lobby_layout_matches_ffximain_tables() {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let Ok(file) = index.read(LOBBY_US) else { return };
        let ui = UiFile::parse(&file).unwrap();
        assert!(ui.warnings.is_empty(), "{:?}", ui.warnings);
        let lib = ui.lib("menu    lobbywin").unwrap();
        assert_eq!(lib.sprites.len(), 236);
        // Expansion logo table 0x1037bf00 points at single ex*us cells in the right column.
        for i in [0x8d, 0x9d, 0x9f, 0xac, 0xad, 0xae, 0xaf, 0xb1, 0xb2, 0xb3, 0xb4] {
            let c = &lib.sprites[i].cells;
            assert_eq!(c.len(), 1);
            assert!(c[0].texture.starts_with("menu    ex"), "{i}: {}", c[0].texture);
            assert_eq!(c[0].corners[0][0], 768);
        }
        // Title buttons: loby2win child refs are the 1-based ids of 0x1037beec minus one.
        let m = ui.menu("menu    loby2win").unwrap();
        let refs: Vec<u16> = m.elements[1..].iter().map(|e| e.refs[0].index).collect();
        assert_eq!(refs, [126, 128, 130, 132, 183]);
    }
}
