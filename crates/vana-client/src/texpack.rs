//! An upscaled texture pack from the game's images (not retail; a mod made by us):
//! `tex-export` saves every stock IMG chunk once (by content) as a PNG with a manifest of where each one goes;
//! an upscaler (Upscayl) enlarges that folder; `tex-pack` rebuilds each file with its images at the new size as DXT,
//! into a mod folder laid out like the game (dat/overlay.rs merges its images by name).

use crate::dat::{self, zone::Texture};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Images drawn by pixel cells (menus, fonts, the Primer's pictures, maps): their files are left out.
const SKIP_PREFIXES: [&str; 5] = ["menu", "font", "cntg", "map", "ex3_"];
/// The largest side the client reads (dat::zone::parse_img).
const MAX_SIDE: u32 = 4096;
/// A chunk's size field: 19 bits of 16-byte units (dat::chunks).
const MAX_CHUNK: usize = 0x7FFFF * 16;
/// Threads for the CPU work (cores 12..15 stay free for the LSB server).
const THREADS: usize = 12;

/// A file's IMG chunks: (chunk index, the chunk's payload). Textures of the file's sprite libraries (chunk 0x31) are
/// left out: their cells address them in stock texels, so a bigger copy would show a quarter of itself.
fn images(bytes: &[u8]) -> Vec<(usize, &[u8])> {
    let Ok(chunks) = dat::chunks(bytes) else { return Vec::new() };
    let sprite_tex: std::collections::HashSet<String> = chunks
        .iter()
        .filter(|c| c.kind == 0x31)
        .filter_map(|c| dat::ui::parse_sprite_lib(c.data).ok())
        .flat_map(|l| l.textures)
        .collect();
    chunks
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            c.kind == 0x20 && !sprite_tex.contains(&dat::ui::norm_name(c.data.get(1..17).unwrap_or_default()))
        })
        .map(|(k, c)| (k, c.data))
        .collect()
}

fn image_name(p: &[u8]) -> String {
    String::from_utf8_lossy(p.get(1..17).unwrap_or_default()).into_owned()
}

/// FNV-1a over the size and pixels: the key an image is saved under once.
fn key(t: &Texture) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in t.width.to_le_bytes().iter().chain(&t.height.to_le_bytes()).chain(t.rgba()) {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn save_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<()> {
    let tmp = path.with_extension("part");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(&tmp)?), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_compression(png::Compression::Fast);
    enc.write_header()?.write_image_data(rgba)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn load_png(path: &Path) -> Result<(u32, u32, Vec<u8>)> {
    let mut d = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path)?));
    d.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut r = d.read_info()?;
    let mut buf = vec![0u8; r.output_buffer_size().context("png size")?];
    let info = r.next_frame(&mut buf)?;
    buf.truncate(info.buffer_size());
    anyhow::ensure!(info.color_type == png::ColorType::Rgba, "{}: not RGBA", path.display());
    Ok((info.width, info.height, buf))
}

/// `status-dump`: the stock 32 x 32 status icons as `<id>.png` and status.tsv (id, name, description, icon key; ids
/// sharing a key share the image), the material for redrawing them.
pub fn status_dump(out: &Path) -> Result<()> {
    let cfg = crate::config::Config::load();
    let index = dat::DatIndex::open_stock(&cfg.ffxi_dir())?;
    let dll = std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir()))?;
    let file = dat::msg::current_slot_file(&dll, 1, 0x3a).context("status info slot")?;
    let bytes = index.read(file)?;
    let names = dat::battle::BattleText::load(&index, &dll)?;
    std::fs::create_dir_all(out)?;
    let mut tsv = std::fs::File::create(out.join("status.tsv"))?;
    writeln!(tsv, "id\tname\tdescription\ticon")?;
    let mut n = 0;
    for id in 0..(bytes.len() / dat::statusinfo::ENTRY) as u16 {
        let Some((w, h, rgba)) = dat::statusinfo::icon(&bytes, id) else { continue };
        // A fully clear icon is an unused slot.
        if rgba.chunks(4).all(|p| p[3] == 0) {
            continue;
        }
        let t = Texture::new(String::new(), 0, w, h, rgba);
        save_png(&out.join(format!("{id}.png")), w, h, t.rgba())?;
        let desc = dat::statusinfo::entry(&bytes, id).map(|e| e.desc).unwrap_or_default();
        writeln!(tsv, "{id}\t{}\t{}\t{}", names.status_title(id), desc.replace(['\t', '\n'], " "), key(&t))?;
        n += 1;
    }
    println!("{n} status icons from file {file} in {}", out.display());
    Ok(())
}

/// `action-icon-dump`: every spell and ability list icon used (magiconw / magico2w) as `<lib>_<index>.png`, and
/// actions.tsv (spell or ability, id, name, type, element, skill, standard and HD icon), the material for redrawing them.
pub fn action_icon_dump(out: &Path) -> Result<()> {
    use crate::dat::names::NameSource;
    let cfg = crate::config::Config::load();
    let index = dat::DatIndex::open_stock(&cfg.ffxi_dir())?;
    let acts = dat::actions::Actions::load(&index)?;
    let names = dat::names::Names::load(&index, dat::items::Lang::En)?;
    let mut ui = dat::ui::UiFile::parse(&index.read(dat::ui::ICONS)?)?;
    ui.absorb(dat::ui::UiFile::parse(&index.read(dat::ui::ICONS2)?)?, true);
    // magic.rs list_icon: HD from 1000 in magico2w, 0xffff none.
    let hd = |icon: u16, hd: u16| match hd {
        0xffff => format!("magiconw_{icon}"),
        1000.. => format!("magico2w_{}", hd - 1000),
        _ => format!("magiconw_{hd}"),
    };
    std::fs::create_dir_all(out)?;
    let mut tsv = std::fs::File::create(out.join("actions.tsv"))?;
    writeln!(tsv, "what\tid\tname\ttype\telement\tskill\tstandard\thd")?;
    let mut used = std::collections::BTreeSet::new();
    for s in acts.spells.iter().filter(|s| s.id != 0) {
        let name = names.spell(s.id as u32).unwrap_or_default();
        let (std_key, hd_key) = (format!("magiconw_{}", s.icon), hd(s.icon, s.icon_hd));
        writeln!(tsv, "spell\t{}\t{name}\t{}\t{}\t{}\t{std_key}\t{hd_key}", s.id, s.kind, s.element, s.skill)?;
        used.extend([std_key, hd_key]);
    }
    for (i, a) in acts.abilities.iter().enumerate() {
        let name = names.ability(i as u32).unwrap_or_default();
        let (std_key, hd_key) = (format!("magiconw_{}", a.icon), hd(a.icon as u16, a.icon_hd));
        writeln!(tsv, "ability\t{i}\t{name}\t{}\t\t\t{std_key}\t{hd_key}", a.kind)?;
        used.extend([std_key, hd_key]);
    }
    let mut keys = std::fs::File::create(out.join("images.tsv"))?;
    writeln!(keys, "image\twidth\theight\tcontent")?;
    for k in &used {
        let (lib, i) = k.split_once('_').context("key")?;
        let Some((w, h, rgba)) = ui.sprite_rgba(&format!("menu    {lib}"), i.parse()?) else { continue };
        let t = Texture::new(String::new(), 0, w, h, rgba);
        save_png(&out.join(format!("{k}.png")), w, h, t.rgba())?;
        writeln!(keys, "{k}\t{w}\t{h}\t{}", key(&t))?;
    }
    println!(
        "{} images for {} spells and {} abilities in {}",
        used.len(),
        acts.spells.iter().filter(|s| s.id != 0).count(),
        acts.abilities.len(),
        out.display()
    );
    Ok(())
}

/// `tex-export`: every stock file's images (files with a skipped prefix left out) once each as `<key>.png` in `out`,
/// and `manifest.tsv` (file id, chunk index, key, name).
pub fn export(out: &Path) -> Result<()> {
    let cfg = crate::config::Config::load();
    let index = dat::DatIndex::open_stock(&cfg.ffxi_dir())?;
    std::fs::create_dir_all(out)?;
    let n = index.len() as u32;
    let next = std::sync::atomic::AtomicU32::new(0);
    let rows = Mutex::new(Vec::new());
    let seen: Mutex<std::collections::HashSet<String>> = Mutex::new(Default::default());
    let (skipped, saved) = (std::sync::atomic::AtomicU32::new(0), std::sync::atomic::AtomicU32::new(0));
    std::thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                loop {
                    let id = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if id >= n {
                        break;
                    }
                    let Ok(bytes) = index.read(id) else { continue };
                    let imgs = images(&bytes);
                    if imgs.is_empty() {
                        continue;
                    }
                    if imgs.iter().any(|(_, p)| SKIP_PREFIXES.iter().any(|x| image_name(p).starts_with(x))) {
                        skipped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        continue;
                    }
                    for (k, p) in imgs {
                        let Ok(t) = dat::zone::parse_img(p) else { continue };
                        if t.width * 4 > MAX_SIDE || t.height * 4 > MAX_SIDE {
                            continue;
                        }
                        let key = key(&t);
                        rows.lock().unwrap().push(format!("{id}\t{k}\t{key}\t{}", t.name.trim()));
                        if seen.lock().unwrap().insert(key.clone()) {
                            let path = out.join(format!("{key}.png"));
                            if !path.exists() && save_png(&path, t.width, t.height, t.rgba()).is_ok() {
                                saved.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    }
                }
            });
        }
    });
    let mut rows = rows.into_inner().unwrap();
    rows.sort_by_key(|r| {
        r.split('\t').take(2).map(|v| v.parse::<u64>().unwrap_or(0)).fold(0, |a, v| a * 1_000_000 + v)
    });
    let mut f = std::io::BufWriter::new(std::fs::File::create(out.join("manifest.tsv"))?);
    for r in &rows {
        writeln!(f, "{r}")?;
    }
    println!(
        "{} image chunks, {} unique saved now, {} files skipped (UI, fonts, maps)",
        rows.len(),
        saved.into_inner(),
        skipped.into_inner()
    );
    Ok(())
}

/// An IMG chunk's payload at the upscaled size: the stock header (name, flags) with the new width and height, DXT1
/// for a DXT1 original (its 1-bit alpha), else DXT3 with the PS2-style alpha (0x80 opaque) the client doubles.
#[cfg(test)]
fn image_chunk(stock: &[u8], w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    image_chunk_from(stock, w, h, &encode(is_dxt1(stock), w, h, rgba, None))
}

fn is_dxt1(stock: &[u8]) -> bool {
    stock[0] == 0xA1 && &stock[57..61] == b"1TXD"
}

/// The pixels as DXT1, or DXT3 with the alpha halved. A translucent image is dithered onto DXT3's levels, or with
/// `levels` (the stock image's few, halved, with their shares) quantized to them in the same shares: the faintest
/// pixels take the lowest level, so specks the upscaler filled in stay clear.
fn encode(dxt1: bool, w: u32, h: u32, rgba: &[u8], levels: Option<&[(u8, f32)]>) -> Vec<u8> {
    let format = if dxt1 { texpresso::Format::Bc1 } else { texpresso::Format::Bc2 };
    let mut px = rgba.to_vec();
    if !dxt1 {
        for p in px.as_chunks_mut::<4>().0 {
            p[3] = (p[3] as u16).div_ceil(2) as u8;
        }
        if translucent(&px) {
            match levels {
                Some(l) if !l.is_empty() => {
                    let mut sorted: Vec<u8> = px.as_chunks::<4>().0.iter().map(|p| p[3]).collect();
                    sorted.sort_unstable();
                    let n = sorted.len();
                    let mut acc = 0.0;
                    let cuts: Vec<u8> = l[..l.len() - 1]
                        .iter()
                        .map(|&(_, share)| {
                            acc += share;
                            sorted[((acc * n as f32) as usize).min(n - 1)]
                        })
                        .collect();
                    for p in px.as_chunks_mut::<4>().0 {
                        p[3] = l[cuts.iter().position(|&c| p[3] < c).unwrap_or(l.len() - 1)].0;
                    }
                }
                _ => dither_alpha(&mut px, w as usize),
            }
        }
    }
    let mut data = vec![0u8; format.compressed_size(w as usize, h as usize)];
    let params = texpresso::Params {
        algorithm: texpresso::Algorithm::ClusterFit,
        weigh_colour_by_alpha: !dxt1,
        ..Default::default()
    };
    format.compress(&px, w as usize, h as usize, params, &mut data);
    data
}

/// The stock image's alpha levels (halved as encode halves) and each one's share of the pixels, when it has at most
/// three: art drawn in a few flat levels (a river's wash with clear specks) that dither would turn to noise once
/// upscaled.
fn stock_levels(stock: &[u8]) -> Option<Vec<(u8, f32)>> {
    let t = dat::zone::parse_img(stock).ok()?;
    let mut count = std::collections::BTreeMap::new();
    for p in t.rgba().as_chunks::<4>().0 {
        *count.entry((p[3] as u16).div_ceil(2) as u8).or_insert(0usize) += 1;
    }
    let n = (t.rgba().len() / 4).max(1) as f32;
    (count.len() <= 3).then(|| count.into_iter().map(|(v, c)| (v, c as f32 / n)).collect())
}

/// Half or more of the visible pixels have partial alpha (glows, smoke), not a cutout's 0 / full.
fn translucent(px: &[u8]) -> bool {
    let (mut vis, mut mid) = (0usize, 0usize);
    for p in px.as_chunks::<4>().0 {
        if p[3] > 5 {
            vis += 1;
            mid += (p[3] > 10 && p[3] < 118) as usize;
        }
    }
    vis > 0 && mid * 2 >= vis
}

/// Floyd-Steinberg onto DXT3's 16 alpha levels, so a smooth gradient does not band at 4 bits.
fn dither_alpha(px: &mut [u8], w: usize) {
    let h = px.len() / 4 / w.max(1);
    let mut err = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let v = px[i * 4 + 3] as f32 + err[i];
            let q = ((v / 17.0).round().clamp(0.0, 15.0)) * 17.0;
            px[i * 4 + 3] = q as u8;
            let e = v - q;
            if x + 1 < w {
                err[i + 1] += e * 7.0 / 16.0;
            }
            if y + 1 < h {
                if x > 0 {
                    err[i + w - 1] += e * 3.0 / 16.0;
                }
                err[i + w] += e * 5.0 / 16.0;
                if x + 1 < w {
                    err[i + w + 1] += e / 16.0;
                }
            }
        }
    }
}

/// The chunk payload around encoded `data`.
fn image_chunk_from(stock: &[u8], w: u32, h: u32, data: &[u8]) -> Vec<u8> {
    let dxt1 = is_dxt1(stock);
    let (kind, pitch) = if dxt1 { (b"1TXD", w * 2) } else { (b"3TXD", w * 4) };
    let size = data.len();
    let mut out = stock[..57].to_vec();
    out[0] = 0xA1;
    out[21..25].copy_from_slice(&w.to_le_bytes());
    out[25..29].copy_from_slice(&h.to_le_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&pitch.to_le_bytes());
    out.extend_from_slice(data);
    out
}

/// An image's encoding, made once per key and format (files share images): `cache/<key>.bc1|bc2|bc2q` (bc2q: alpha
/// quantized to the stock levels) holds w, h, data.
fn encoded(
    cache: &Path,
    key: &str,
    png: &Path,
    dxt1: bool,
    levels: Option<&[(u8, f32)]>,
) -> Result<(u32, u32, Vec<u8>)> {
    let path = cache.join(format!(
        "{key}.{}",
        if dxt1 {
            "bc1"
        } else if levels.is_some() {
            "bc2q"
        } else {
            "bc2"
        }
    ));
    if let Ok(b) = std::fs::read(&path)
        && b.len() > 8
    {
        return Ok((u32::from_le_bytes(b[0..4].try_into()?), u32::from_le_bytes(b[4..8].try_into()?), b[8..].to_vec()));
    }
    let (mut w, mut h, mut rgba) = load_png(png)?;
    anyhow::ensure!(w <= MAX_SIDE && h <= MAX_SIDE, "{key}: {w}x{h} too large");
    // Halved until its chunk fits (4x of a 512 x 1024 DXT3 is past 8 MB).
    let format = if dxt1 { texpresso::Format::Bc1 } else { texpresso::Format::Bc2 };
    while 16 + 69 + format.compressed_size(w as usize, h as usize) > MAX_CHUNK && w > 4 && h > 4 {
        (w, h, rgba) = half(w, h, &rgba);
    }
    let data = encode(dxt1, w, h, &rgba, levels);
    let mut b = Vec::with_capacity(8 + data.len());
    b.extend_from_slice(&w.to_le_bytes());
    b.extend_from_slice(&h.to_le_bytes());
    b.extend_from_slice(&data);
    let tmp = path.with_extension(format!("part{:?}", std::thread::current().id()).replace(['(', ')'], ""));
    if std::fs::write(&tmp, &b).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
    Ok((w, h, data))
}

/// A 2x2 box filter.
fn half(w: u32, h: u32, rgba: &[u8]) -> (u32, u32, Vec<u8>) {
    let (nw, nh) = (w / 2, h / 2);
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh as usize {
        for x in 0..nw as usize {
            for c in 0..4 {
                let at = |dx: usize, dy: usize| rgba[((2 * y + dy) * w as usize + 2 * x + dx) * 4 + c] as u32;
                out[(y * nw as usize + x) * 4 + c] = ((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8;
            }
        }
    }
    (nw, nh, out)
}

/// A chunk with `payload` (the stock chunk's 16-byte header, its size word redone; padded to 16 bytes).
fn chunk_bytes(stock_header: &[u8], payload: &[u8]) -> Vec<u8> {
    let total = (16 + payload.len()).div_ceil(16) * 16;
    assert!(total <= MAX_CHUNK, "chunk of {total} bytes past the size field");
    let mut out = stock_header[..16].to_vec();
    let word = u32::from_le_bytes(out[4..8].try_into().unwrap());
    let word = (word & 0x7f) | (((total / 16) as u32) << 7);
    out[4..8].copy_from_slice(&word.to_le_bytes());
    out.extend_from_slice(payload);
    out.resize(total, 0);
    out
}

/// `tex-pack`: each manifest file rebuilt from the stock one with its upscaled images (`up`: `<key>.png`, any upscaler's
/// name suffix allowed) into `out/ROM*/dir/file.DAT`. Files already built are kept, but those holding a key listed in
/// `redo` (one per line, first column), and every encoding is cached in `<manifest>/encoded`, so it can resume.
pub fn pack(manifest_dir: &Path, up: &Path, out: &Path, redo: Option<&Path>) -> Result<()> {
    let cfg = crate::config::Config::load();
    let index = dat::DatIndex::open_stock(&cfg.ffxi_dir())?;
    let stock_root = cfg.ffxi_dir();
    // The upscaled files by key (the name up to the first '_' or '.').
    let mut by_key: HashMap<String, PathBuf> = HashMap::new();
    for e in std::fs::read_dir(up)?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.ends_with(".png") {
            by_key.insert(name.split(['_', '.']).next().unwrap_or_default().to_string(), e.path());
        }
    }
    let mut files: Vec<(u32, Vec<(usize, String)>)> = Vec::new();
    for line in std::fs::read_to_string(manifest_dir.join("manifest.tsv"))?.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let (Ok(id), Ok(k)) = (f[0].parse::<u32>(), f[1].parse::<usize>()) else { continue };
        match files.last_mut() {
            Some((last, v)) if *last == id => v.push((k, f[2].to_string())),
            _ => files.push((id, vec![(k, f[2].to_string())])),
        }
    }
    // Ids sharing a file: built once.
    let mut paths = std::collections::HashSet::new();
    files.retain(|(id, _)| index.path(*id).is_some_and(|p| paths.insert(p)));
    let redo: std::collections::HashSet<String> = match redo {
        Some(p) => {
            std::fs::read_to_string(p)?.lines().filter_map(|l| l.split('\t').next()).map(str::to_string).collect()
        }
        None => Default::default(),
    };
    let cache = manifest_dir.join("encoded");
    std::fs::create_dir_all(&cache)?;
    let next = std::sync::atomic::AtomicUsize::new(0);
    let (built, missing) = (std::sync::atomic::AtomicU32::new(0), std::sync::atomic::AtomicU32::new(0));
    std::thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((id, wanted)) = files.get(i) else { break };
                    let Some(stock) = index.path(*id) else { continue };
                    let Ok(rel) = stock.strip_prefix(&stock_root) else { continue };
                    let dest = out.join(rel);
                    if dest.exists() && !wanted.iter().any(|(_, key)| redo.contains(key)) {
                        continue;
                    }
                    let Ok(bytes) = std::fs::read(&stock) else { continue };
                    let Ok(chunks) = dat::chunks(&bytes) else { continue };
                    let allowed: std::collections::HashSet<usize> =
                        images(&bytes).into_iter().map(|(k, _)| k).collect();
                    let keys: HashMap<usize, &String> =
                        wanted.iter().filter(|(k, _)| allowed.contains(k)).map(|(k, key)| (*k, key)).collect();
                    let mut outb = Vec::with_capacity(bytes.len() * 8);
                    let mut changed = false;
                    for (k, c) in chunks.iter().enumerate() {
                        let header = &bytes[c.offset..c.offset + 16];
                        let end = c.offset + 16 + c.data.len();
                        let up = keys.get(&k).and_then(|key| by_key.get(*key).map(|p| (key, p)));
                        match up
                            .map(|(key, p)| encoded(&cache, key, p, is_dxt1(c.data), stock_levels(c.data).as_deref()))
                        {
                            Some(Ok((w, h, data))) => {
                                outb.extend(chunk_bytes(header, &image_chunk_from(c.data, w, h, &data)));
                                changed = true;
                            }
                            _ => {
                                if keys.contains_key(&k) {
                                    missing.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                }
                                outb.extend_from_slice(&bytes[c.offset..end]);
                            }
                        }
                    }
                    if !changed {
                        continue;
                    }
                    if let Some(dir) = dest.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    let tmp = dest.with_extension("part");
                    if std::fs::write(&tmp, &outb).is_ok() && std::fs::rename(&tmp, &dest).is_ok() {
                        let n = built.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        if n % 500 == 0 {
                            println!("{n} files built");
                        }
                    }
                }
            });
        }
    });
    println!("{} files built, {} images had no upscaled copy (kept stock)", built.into_inner(), missing.into_inner());
    Ok(())
}

/// `tex-slim`: the pack in `pack` cut to its images for publishing (no stock data in it): each file keeps only the
/// images that differ from the stock file's (dat::overlay::slim), checked to merge to the same bytes as the full copy,
/// written to `out` at the same path. Files written before are left as they are.
pub fn slim(pack: &Path, out: &Path) -> Result<()> {
    let stock_root = crate::config::Config::load().ffxi_dir();
    let mut files = Vec::new();
    let mut dirs = vec![pack.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).with_context(|| format!("reading {}", d.display()))?.flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if let Ok(rel) = p.strip_prefix(pack) {
                files.push(rel.to_path_buf());
            }
        }
    }
    files.sort();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let counts: Mutex<[u64; 4]> = Mutex::new([0; 4]); // written, unchanged, full bytes, slim bytes
    let failed: Mutex<Vec<String>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(rel) = files.get(i) else { break };
                    let dest = out.join(rel);
                    if dest.exists() {
                        continue;
                    }
                    let (Ok(stock), Ok(full)) = (std::fs::read(stock_root.join(rel)), std::fs::read(pack.join(rel)))
                    else {
                        failed.lock().unwrap().push(format!("{}: unreadable", rel.display()));
                        continue;
                    };
                    let Some((bytes, kept)) = dat::overlay::slim(&stock, &full) else {
                        failed.lock().unwrap().push(format!("{}: not a chunk file", rel.display()));
                        continue;
                    };
                    if kept == 0 {
                        counts.lock().unwrap()[1] += 1;
                        continue;
                    }
                    if dat::overlay::merge(&stock, std::slice::from_ref(&bytes))
                        != dat::overlay::merge(&stock, std::slice::from_ref(&full))
                    {
                        failed.lock().unwrap().push(format!("{}: merges differently", rel.display()));
                        continue;
                    }
                    if let Some(dir) = dest.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    let tmp = dest.with_extension("part");
                    if std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, &dest)).is_err() {
                        failed.lock().unwrap().push(format!("{}: not written", rel.display()));
                        continue;
                    }
                    let mut c = counts.lock().unwrap();
                    (c[0], c[2], c[3]) = (c[0] + 1, c[2] + full.len() as u64, c[3] + bytes.len() as u64);
                    if c[0].is_multiple_of(2000) {
                        println!("{} files written", c[0]);
                    }
                }
            });
        }
    });
    let c = counts.into_inner().unwrap();
    let failed = failed.into_inner().unwrap();
    for f in &failed {
        println!("{f}");
    }
    println!(
        "{} files written ({:.1} GB of {:.1} GB), {} with no image changed, {} failed",
        c[0],
        c[3] as f64 / 1e9,
        c[2] as f64 / 1e9,
        c[1],
        failed.len()
    );
    Ok(())
}

/// A PNG of any colour type as RGBA8 (grey into all three channels).
fn load_any_png(path: &Path) -> Result<(u32, u32, Vec<u8>)> {
    let mut d = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path)?));
    d.set_transformations(png::Transformations::normalize_to_color8());
    let mut r = d.read_info()?;
    let mut buf = vec![0u8; r.output_buffer_size().context("png size")?];
    let info = r.next_frame(&mut buf)?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => {
            buf.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect()
        }
        c => anyhow::bail!("{}: {c:?}", path.display()),
    };
    Ok((info.width, info.height, rgba))
}

/// A material map image as BC5 with its mips down to 4 texels; `normal` renormalises each level (x, y kept, z from the
/// blue channel).
fn bc5_chain(w: u32, h: u32, rgba: &[u8], levels: u32, normal: bool) -> Vec<u8> {
    let mut px: Vec<[f32; 3]> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let v = [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0];
            if normal { [v[0] * 2.0 - 1.0, v[1] * 2.0 - 1.0, v[2] * 2.0 - 1.0] } else { v }
        })
        .collect();
    let (mut lw, mut lh) = (w as usize, h as usize);
    let mut out = Vec::with_capacity(crate::material::Material::chain_size(w, h, levels));
    for l in 0..levels {
        if l > 0 {
            let (nw, nh) = (lw / 2, lh / 2);
            let mut next = vec![[0f32; 3]; nw * nh];
            for y in 0..nh {
                for x in 0..nw {
                    let mut s = [0f32; 3];
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let p = px[(2 * y + dy) * lw + 2 * x + dx];
                        for c in 0..3 {
                            s[c] += p[c] * 0.25;
                        }
                    }
                    next[y * nw + x] = s;
                }
            }
            (px, lw, lh) = (next, nw, nh);
        }
        let enc: Vec<u8> = px
            .iter()
            .flat_map(|p| {
                let q = if normal {
                    let n = (p[0] * p[0] + p[1] * p[1] + p[2].max(0.0) * p[2].max(0.0)).sqrt().max(1e-6);
                    [p[0] / n * 0.5 + 0.5, p[1] / n * 0.5 + 0.5]
                } else {
                    [p[0], p[1]]
                };
                [(q[0] * 255.0 + 0.5).clamp(0.0, 255.0) as u8, (q[1] * 255.0 + 0.5).clamp(0.0, 255.0) as u8, 0, 255]
            })
            .collect();
        let mut data = vec![0u8; texpresso::Format::Bc5.compressed_size(lw, lh)];
        texpresso::Format::Bc5.compress(&enc, lw, lh, Default::default(), &mut data);
        out.extend_from_slice(&data);
    }
    out
}

/// One image's maps from `maps/<key>.n.png`, `.r.png` and (equipment only) `.m.png`; None when they are missing or
/// not a multiple of 4.
fn material_from(maps: &Path, key: &str) -> Option<crate::material::Material> {
    let (w, h, n) = load_any_png(&maps.join(format!("{key}.n.png"))).ok()?;
    let (rw, rh, r) = load_any_png(&maps.join(format!("{key}.r.png"))).ok()?;
    if (rw, rh) != (w, h) || w % 4 != 0 || h % 4 != 0 {
        return None;
    }
    let m = load_any_png(&maps.join(format!("{key}.m.png"))).ok().filter(|m| (m.0, m.1) == (w, h)).map(|m| m.2);
    let rm: Vec<u8> =
        (0..(w * h) as usize).flat_map(|i| [r[i * 4], m.as_ref().map_or(0, |m| m[i * 4]), 0, 255]).collect();
    let mut levels = 1;
    while (w >> levels) >= 4 && (h >> levels) >= 4 {
        levels += 1;
    }
    Some(crate::material::Material {
        width: w,
        height: h,
        levels,
        normal: bc5_chain(w, h, &n, levels, true),
        rough_metal: bc5_chain(w, h, &rm, levels, false),
    })
}

/// `mat-pack`: the material pack (material.rs) from generated maps (`maps/<key>.n.png`, `.r.png`, `.m.png`) for the
/// images in tex-export's manifest: each map once as `out/maps/../<key>.vmat`, and `out/index.bin` holding every
/// stock image's content key and, with `hd` (a tex-pack folder), its copy's there. Maps already written are kept.
pub fn mat_pack(manifest_dir: &Path, maps: &Path, hd: Option<&Path>, out: &Path) -> Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    let cfg = crate::config::Config::load();
    let index = dat::DatIndex::open_stock(&cfg.ffxi_dir())?;
    let stock_root = cfg.ffxi_dir();
    let manifest = std::fs::read_to_string(manifest_dir.join("manifest.tsv"))?;
    let mut by_file: HashMap<u32, Vec<(usize, String)>> = HashMap::new();
    for l in manifest.lines() {
        let f: Vec<&str> = l.split('\t').collect();
        let (Some(id), Some(k), Some(key)) = (f.first(), f.get(1), f.get(2)) else { continue };
        by_file.entry(id.parse()?).or_default().push((k.parse()?, key.to_string()));
    }
    let mut keys: Vec<String> = by_file.values().flatten().map(|(_, k)| k.clone()).collect();
    keys.sort();
    keys.dedup();
    // The maps, once each.
    let (next, made, missing) = (AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0));
    let have: Mutex<std::collections::HashSet<String>> = Mutex::new(Default::default());
    std::thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Relaxed);
                    let Some(key) = keys.get(i) else { break };
                    let Ok(k) = u64::from_str_radix(key, 16) else { continue };
                    let path = crate::material::map_path(out, k);
                    if !path.exists() {
                        let Some(m) = material_from(maps, key) else {
                            missing.fetch_add(1, Relaxed);
                            continue;
                        };
                        let _ = std::fs::create_dir_all(path.parent().unwrap());
                        let tmp = path.with_extension("part");
                        if std::fs::write(&tmp, m.write()).is_err() || std::fs::rename(&tmp, &path).is_err() {
                            continue;
                        }
                        made.fetch_add(1, Relaxed);
                    }
                    have.lock().unwrap().insert(key.clone());
                    if (i + 1) % 2000 == 0 {
                        println!("maps: {} / {}", i + 1, keys.len());
                    }
                }
            });
        }
    });
    let have = have.into_inner().unwrap();
    // The index: each image's content key, stock and (with `hd`) the pack's copy, to its map.
    let files: Vec<(&u32, &Vec<(usize, String)>)> = by_file.iter().collect();
    let next = AtomicUsize::new(0);
    let entries = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Relaxed);
                    let Some(&(&id, rows)) = files.get(i) else { break };
                    if !rows.iter().any(|(_, k)| have.contains(k)) {
                        continue;
                    }
                    let mut versions = Vec::new();
                    if let Ok(b) = index.read(id) {
                        versions.push(b);
                    }
                    if let Some(hd) = hd
                        && let Some(p) = index.path(id)
                        && let Ok(rel) = p.strip_prefix(&stock_root)
                        && let Ok(b) = std::fs::read(hd.join(rel))
                    {
                        versions.push(b);
                    }
                    let mut found = Vec::new();
                    for b in &versions {
                        let Ok(chunks) = dat::chunks(b) else { continue };
                        for (k, key) in rows {
                            if !have.contains(key) {
                                continue;
                            }
                            let Some(c) = chunks.get(*k).filter(|c| c.kind == 0x20) else { continue };
                            if let (Ok(t), Ok(m)) = (dat::zone::parse_img(c.data), u64::from_str_radix(key, 16)) {
                                found.push((t.content_key(), m));
                            }
                        }
                    }
                    entries.lock().unwrap().extend(found);
                }
            });
        }
    });
    let mut entries = entries.into_inner().unwrap();
    entries.sort_unstable();
    entries.dedup();
    let before = entries.len();
    entries.dedup_by_key(|e| e.0);
    let mut b = Vec::with_capacity(8 + entries.len() * 16);
    b.extend_from_slice(crate::material::INDEX_MAGIC);
    b.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (c, m) in &entries {
        b.extend_from_slice(&c.to_le_bytes());
        b.extend_from_slice(&m.to_le_bytes());
    }
    std::fs::create_dir_all(out)?;
    std::fs::write(out.join("index.bin"), b)?;
    println!(
        "{} maps ({} new, {} without maps), index of {} textures ({} keys mapped twice dropped) in {}",
        have.len(),
        made.into_inner(),
        missing.into_inner(),
        entries.len(),
        before - entries.len(),
        out.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_round_trip() {
        // A 4x4 DXT3 stock image becomes an 8x8 one the client reads back at that size.
        let mut stock = vec![0u8; 69 + 16];
        stock[0] = 0xA1;
        stock[1..17].copy_from_slice(b"tim     testtex ");
        stock[21..25].copy_from_slice(&4u32.to_le_bytes());
        stock[25..29].copy_from_slice(&4u32.to_le_bytes());
        stock[31] = 8;
        stock[57..61].copy_from_slice(b"3TXD");
        stock[61..65].copy_from_slice(&16u32.to_le_bytes());
        let rgba: Vec<u8> = (0..64).flat_map(|i| [i as u8 * 4, 128, 255 - i as u8 * 4, 255]).collect();
        let p = image_chunk(&stock, 8, 8, &rgba);
        let t = dat::zone::parse_img(&p).unwrap();
        assert_eq!((t.width, t.height, t.name.trim()), (8, 8, "tim     testtex".trim()));
        assert!(t.rgba().chunks(4).all(|px| px[3] >= 254));
        let header = [b'i', b'm', b'g', b'1', 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let c = chunk_bytes(&header, &p);
        assert_eq!(c.len() % 16, 0);
        let parsed = dat::chunks(&c).unwrap();
        assert_eq!((parsed[0].kind, parsed[0].data.len() >= p.len()), (0x20, true));
    }
}
