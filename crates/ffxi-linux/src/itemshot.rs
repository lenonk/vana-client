//! `item-dump` and `item-icon-shot`: item records (dat/items.rs) as text and icon sheets.

use crate::dat::DatIndex;
use crate::dat::items::{Items, Lang};
use anyhow::{Context, Result};

fn open(lang: &str) -> Result<Items> {
    let index = DatIndex::open(&crate::config::Config::load().ffxi_dir())?;
    Items::load(&index, Lang::parse(lang).context("lang: ja, en, de or fr")?)
}

const JOBS: [&str; 23] = [
    "-", "WAR", "MNK", "WHM", "BLM", "RDM", "THF", "PLD", "DRK", "BST", "BRD", "RNG", "SAM", "NIN", "DRG", "SMN", "BLU", "COR", "PUP", "DNC", "SCH", "GEO", "RUN",
];

pub fn dump(lang: &str, key: bool, ids: &[u32]) -> Result<()> {
    if key {
        use crate::dat::names::{NameSource, Names};
        let index = DatIndex::open(&crate::config::Config::load().ffxi_dir())?;
        let names = Names::load(&index, Lang::parse(lang).context("lang: ja, en, de or fr")?)?;
        for &id in ids {
            match names.key_item(id) {
                Some(n) => println!("key item {id}: \"{}\" / \"{}\" (mode {}, an {})", n.singular, n.plural, n.mode, n.an),
                None => println!("key item {id}: none"),
            }
            for l in names.key_item_description(id).unwrap_or_default().lines() {
                println!("  | {l}");
            }
        }
        return Ok(());
    }
    let items = open(lang)?;
    for &id in ids {
        let Some(it) = items.item(id) else {
            println!("{id:#06x}: none");
            continue;
        };
        let jobs: Vec<&str> = (1..23).filter(|&j| it.jobs & (1 << j) != 0).map(|j| JOBS[j]).collect();
        println!("{id:#06x} {:?} \"{}\"", it.kind.unwrap(), it.name);
        println!("  flags {:#06x} stack {} type {} resource {:#06x} targets {:#x}", it.flags, it.stack, it.item_type, it.resource, it.targets);
        if it.level != 0 || it.slots != 0 {
            println!("  level {} slots {:?} races {:#x} jobs {} superior {}", it.level, it.slot_names(), it.races, jobs.join("/"), it.superior_level);
        }
        if it.damage != 0 || it.delay != 0 {
            println!("  damage {} delay {} dps {} skill {}", it.damage, it.delay, it.dps, it.skill);
        }
        if it.max_charges != 0 {
            println!("  charges {} cast {} use delay {} reuse {}", it.max_charges, it.cast_time, it.use_delay, it.reuse_delay);
        }
        if !it.singular.is_empty() {
            println!("  log: article {} \"{}\" / \"{}\"", it.article, it.singular, it.plural);
        }
        if !it.extra.is_empty() {
            println!("  extra {}", it.extra.iter().map(|(o, v)| format!("{o:#x}={v}")).collect::<Vec<_>>().join(" "));
        }
        for l in it.description.lines() {
            println!("  | {l}");
        }
        println!("  icon {}", it.icon_rgba().map_or("none".into(), |i| format!("{}x{}", i.0, i.1)));
    }
    Ok(())
}

/// Icons at 4x in a row of up to 8 per line, on a grey background.
pub fn icon_shot(ids: &[u32], out: &std::path::Path, lang: &str) -> Result<()> {
    let items = open(lang)?;
    let (cell, zoom, per) = (32u32 + 8, 4u32, 8usize);
    let cols = ids.len().clamp(1, per) as u32;
    let rows = ids.len().div_ceil(per).max(1) as u32;
    let (w, h) = (cols * cell * zoom, rows * cell * zoom);
    let mut img: Vec<u8> = (0..w * h).flat_map(|_| [64u8, 64, 72, 255]).collect();
    for (n, &id) in ids.iter().enumerate() {
        let Some((iw, ih, px)) = items.icon(id) else {
            println!("{id:#06x}: no icon");
            continue;
        };
        let (ox, oy) = ((n % per) as u32 * cell * zoom + 4 * zoom, (n / per) as u32 * cell * zoom + 4 * zoom);
        for y in 0..ih * zoom {
            for x in 0..iw * zoom {
                let s = (((y / zoom) * iw + x / zoom) * 4) as usize;
                let d = (((oy + y) * w + ox + x) * 4) as usize;
                let a = px[s + 3] as u32;
                for c in 0..3 {
                    img[d + c] = ((px[s + c] as u32 * a + img[d + c] as u32 * (255 - a)) / 255) as u8;
                }
            }
        }
        println!("{id:#06x} {}", items.item(id).map(|i| i.name).unwrap_or_default());
    }
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(out)?), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header()?.write_image_data(&img)?;
    println!("wrote {}x{} to {}", w, h, out.display());
    Ok(())
}
