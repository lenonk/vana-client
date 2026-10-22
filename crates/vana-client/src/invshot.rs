//! `inventory-shot` / `equipment-shot`: the item list, equipment, status and gil windows offscreen,
//! with placeholder items (an `ItemInfo` from the item DATs can replace `Placeholder`).

use crate::config::Config;
use crate::dat::DatIndex;
use crate::dat::font::{Font, Quality};
use crate::dat::ui::{MENU_US, UiFile, UiImage, WINDOW_STYLE_BASE, norm_name};
use crate::inventory::{Equipment, ItemInfo, ItemList, Persona, Slot, Stats, build_rows, gil_text};
use crate::render::invwin::{icon_texture, push_equipment, push_inventory, push_money, push_persona};
use crate::render::ui::{UiBatch, render_png};
use crate::uishot::{load_ui, load_ui_full};
use anyhow::{Context, Result};
use std::path::Path;

const ATLAS: &str = "\0font atlas";

/// Stand-in item data: (id, name, stack, type, flags).
pub struct Placeholder(pub Vec<(u16, &'static str, u16, u16, u16)>);

impl Placeholder {
    pub fn new() -> Self {
        Placeholder(vec![
            (4096, "Fire Crystal", 12, 8, 0),
            (4097, "Ice Crystal", 12, 8, 0),
            (4509, "Distilled Water", 12, 7, 0x0400),
            (4376, "Meat Jerky", 12, 7, 0x0400),
            (4112, "Potion", 12, 7, 0x0400),
            (16465, "Bronze Knife", 1, 4, 0),
            (17307, "Dart", 99, 4, 0),
            (12448, "Bronze Cap", 1, 5, 0),
            (12576, "Bronze Harness", 1, 5, 0),
            (12704, "Bronze Mittens", 1, 5, 0),
            (12832, "Bronze Subligar", 1, 5, 0),
            (12960, "Bronze Leggings", 1, 5, 0),
            (13469, "Leather Ring", 1, 5, 0),
            (640, "Copper Ore", 12, 1, 0),
            (848, "Rabbit Hide", 12, 1, 0),
        ])
    }

    fn get(&self, id: u16) -> Option<&(u16, &'static str, u16, u16, u16)> {
        self.0.iter().find(|e| e.0 == id)
    }
}

impl ItemInfo for Placeholder {
    fn name(&self, id: u16) -> Option<String> {
        self.get(id).map(|e| e.1.to_string())
    }
    fn stack(&self, id: u16) -> u16 {
        self.get(id).map_or(1, |e| e.2)
    }
    fn kind(&self, id: u16) -> u16 {
        self.get(id).map_or(0, |e| e.3)
    }
    fn flags(&self, id: u16) -> u16 {
        self.get(id).map_or(0, |e| e.4)
    }
    /// A framed square tinted by type, so rows and slots read without the item DATs.
    fn icon(&self, id: u16) -> Option<Vec<u8>> {
        let kind = self.kind(id);
        let c: [u8; 3] = match kind {
            4 => [190, 190, 210],
            5 => [170, 120, 70],
            7 => [90, 170, 220],
            8 => [230, 90, 60],
            _ => [140, 160, 110],
        };
        let mut px = vec![0u8; 32 * 32 * 4];
        for y in 0..32 {
            for x in 0..32 {
                let edge = !(3..29).contains(&x) || !(3..29).contains(&y);
                let inside = (2..30).contains(&x) && (2..30).contains(&y);
                let shade = 1.0 - (x + y) as f32 / 96.0;
                let p = &mut px[(y * 32 + x) * 4..][..4];
                if inside {
                    let k = if edge { 0.45 } else { shade };
                    p.copy_from_slice(&[(c[0] as f32 * k) as u8, (c[1] as f32 * k) as u8, (c[2] as f32 * k) as u8, 255]);
                }
            }
        }
        Some(px)
    }
}

fn font_file(font: &Font) -> UiFile {
    let mut f = UiFile::default();
    let a = &font.atlas;
    f.images.insert(ATLAS.into(), UiImage { width: a.width, height: a.height, rgba: a.rgba.clone() });
    for (name, t) in &font.textures {
        f.images.insert(norm_name(name.as_bytes()), UiImage { width: t.width, height: t.height, rgba: t.rgba.clone() });
    }
    f
}

fn icon_file(info: &dyn ItemInfo, ids: impl Iterator<Item = u16>) -> UiFile {
    let mut f = UiFile::default();
    for id in ids {
        if let Some(rgba) = info.icon(id) {
            f.images.insert(icon_texture(id), UiImage { width: 32, height: 32, rgba });
        }
    }
    f
}

struct Shared {
    menu: UiFile,
    style: UiFile,
    font: Font,
    info: Box<dyn ItemInfo>,
}

/// Item data for names and icons: the placeholders unless the item DATs are wired in.
fn item_source(index: &DatIndex) -> Box<dyn ItemInfo> {
    crate::invitems::load(index).unwrap_or_else(|| Box::new(Placeholder::new()))
}

fn shared() -> Result<Shared> {
    let cfg = Config::load();
    let index = DatIndex::open(&cfg.ffxi_dir())?;
    let menu = load_ui_full(&index, MENU_US)?;
    let style = load_ui(&index, WINDOW_STYLE_BASE)?;
    let font = Font::load(&index, &std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?, 1, Quality::Compressed)?;
    let info = item_source(&index);
    Ok(Shared { menu, style, font, info })
}

fn slots(info: &Placeholder) -> Vec<Slot> {
    let mut v = vec![Slot { index: 0, id: 0xffff, count: 45_006, equipped: false, equipped_ex: false }];
    let counts = [12u32, 7, 5, 3, 4, 1, 99, 1, 1, 1, 1, 1, 1, 9, 2];
    for (i, (e, n)) in info.0.iter().zip(counts).enumerate() {
        let equipped = matches!(e.0, 16465 | 17307 | 12448 | 12576);
        v.push(Slot { index: i as u8 + 1, id: e.0, count: n, equipped, equipped_ex: false });
    }
    v
}

pub fn inventory_shot(out: &Path, scroll: usize, cursor: usize, frame: usize, width: u32, height: u32) -> Result<()> {
    let s = shared()?;
    let style = s.style.libs.first().context("window style file has no sprite library")?;
    let slots = slots(&Placeholder::new());
    let info = s.info.as_ref();
    let list = ItemList { rows: build_rows(&slots, info, false), scroll, cursor };
    for r in &list.rows {
        println!("row {:5} {:<20} count {:?} tint {:08x}", r.id, r.name, r.count, r.tint);
    }
    let (w, h) = (width as f32, height as f32);
    let mut b = UiBatch::default();
    let inv = s.menu.menu("menu    inventor").context("no inventor menu")?;
    push_inventory(&mut b, &s.menu, style, &s.font, ATLAS, inv, &list, w, h, frame);
    let money = s.menu.menu("menu    money").context("no money menu")?;
    push_money(&mut b, &s.menu, style, money, &gil_text(slots[0].count as i64), w, h);
    let icons = icon_file(info, list.rows.iter().map(|r| r.id));
    let fonts = font_file(&s.font);
    render_png(&[&s.menu, &fonts, &icons], &b, width, height, [0.30, 0.36, 0.28], out)?;
    println!("wrote {}", out.display());
    Ok(())
}

pub fn equipment_shot(out: &Path, cursor: usize, frame: usize, width: u32, height: u32) -> Result<()> {
    let s = shared()?;
    let style = s.style.libs.first().context("window style file has no sprite library")?;
    let all = slots(&Placeholder::new());
    let info = s.info.as_ref();
    let by_id = |id: u16| all.iter().find(|x| x.id == id).cloned();
    let mut eq = Equipment { fields: [12, -3, 0, 5, 7, 0, 2, -1, 312, 287], ..Default::default() };
    // main, ammo, head, body, hands, legs, feet, ring1
    for (slot, id) in [(0, 16465), (3, 17307), (4, 12448), (5, 12576), (6, 12704), (7, 12832), (8, 12960), (13, 13469)] {
        eq.slots[slot] = by_id(id);
    }
    let p = Persona::build(&Stats {
        name: "Lenon".into(),
        main: Some((75, "WAR".into())),
        sub: Some((37, "NIN".into())),
        ilvl: None,
        su: 0,
        hp: (1234, 1500),
        mp: (0, 0),
        tp: 1000,
        base: [78, 72, 70, 65, 50, 55, 52],
        bonus: [12, 5, 0, -2, 0, 3, 0],
    });
    let (w, h) = (width as f32, height as f32);
    let mut b = UiBatch::default();
    let persona = s.menu.menu("menu    persona").context("no persona menu")?;
    push_persona(&mut b, &s.menu, style, persona, &p, w, h);
    let money = s.menu.menu("menu    money").context("no money menu")?;
    push_money(&mut b, &s.menu, style, money, &gil_text(all[0].count as i64), w, h);
    let equip = s.menu.menu("menu    equip").context("no equip menu")?;
    push_equipment(&mut b, &s.menu, style, equip, &eq, info, w, h, Some(cursor), frame);
    let icons = icon_file(info, eq.slots.iter().flatten().map(|x| x.id));
    let fonts = font_file(&s.font);
    render_png(&[&s.menu, &fonts, &icons], &b, width, height, [0.30, 0.36, 0.28], out)?;
    println!("wrote {}", out.display());
    Ok(())
}
