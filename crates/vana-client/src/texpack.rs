//! An upscaled texture pack from the game's own images (not retail; a mod made by us):
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

/// A file's IMG chunks: (chunk index, the chunk's payload).
fn images(bytes: &[u8]) -> Vec<(usize, &[u8])> {
    dat::chunks(bytes).map_or(Vec::new(), |c| c.iter().enumerate().filter(|(_, c)| c.kind == 0x20).map(|(k, c)| (k, c.data)).collect())
}

fn image_name(p: &[u8]) -> String {
    String::from_utf8_lossy(p.get(1..17).unwrap_or_default()).into_owned()
}

/// FNV-1a over the size and pixels: the key an image is saved under once.
fn key(t: &Texture) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in t.width.to_le_bytes().iter().chain(&t.height.to_le_bytes()).chain(&t.rgba) {
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
    let dll = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?;
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
        let t = Texture { name: String::new(), format: 0, width: w, height: h, rgba, dxt: None };
        save_png(&out.join(format!("{id}.png")), w, h, &t.rgba)?;
        let desc = dat::statusinfo::entry(&bytes, id).map(|e| e.desc).unwrap_or_default();
        writeln!(tsv, "{id}\t{}\t{}\t{}", names.status_title(id), desc.replace(['\t', '\n'], " "), key(&t))?;
        n += 1;
    }
    println!("{n} status icons from file {file} in {}", out.display());
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
                            if !path.exists() && save_png(&path, t.width, t.height, &t.rgba).is_ok() {
                                saved.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    }
                }
            });
        }
    });
    let mut rows = rows.into_inner().unwrap();
    rows.sort_by_key(|r| r.split('\t').take(2).map(|v| v.parse::<u64>().unwrap_or(0)).fold(0, |a, v| a * 1_000_000 + v));
    let mut f = std::io::BufWriter::new(std::fs::File::create(out.join("manifest.tsv"))?);
    for r in &rows {
        writeln!(f, "{r}")?;
    }
    println!("{} image chunks, {} unique saved now, {} files skipped (UI, fonts, maps)", rows.len(), saved.into_inner(), skipped.into_inner());
    Ok(())
}

/// An IMG chunk's payload at the upscaled size: the stock header (name, flags) with the new width and height, DXT1
/// for a DXT1 original (its 1-bit alpha), else DXT3 with the PS2-style alpha (0x80 opaque) the client doubles.
fn image_chunk(stock: &[u8], w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    image_chunk_from(stock, w, h, &encode(is_dxt1(stock), w, h, rgba))
}

fn is_dxt1(stock: &[u8]) -> bool {
    stock[0] == 0xA1 && &stock[57..61] == b"1TXD"
}

/// The pixels as DXT1, or DXT3 with the alpha halved.
fn encode(dxt1: bool, w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let format = if dxt1 { texpresso::Format::Bc1 } else { texpresso::Format::Bc2 };
    let mut px = rgba.to_vec();
    if !dxt1 {
        for p in px.chunks_exact_mut(4) {
            p[3] = (p[3] as u16).div_ceil(2) as u8;
        }
        if translucent(&px) {
            dither_alpha(&mut px, w as usize);
        }
    }
    let mut data = vec![0u8; format.compressed_size(w as usize, h as usize)];
    let params = texpresso::Params { algorithm: texpresso::Algorithm::ClusterFit, weigh_colour_by_alpha: !dxt1, ..Default::default() };
    format.compress(&px, w as usize, h as usize, params, &mut data);
    data
}

/// Half or more of the visible pixels have partial alpha (glows, smoke), not a cutout's 0 / full.
fn translucent(px: &[u8]) -> bool {
    let (mut vis, mut mid) = (0usize, 0usize);
    for p in px.chunks_exact(4) {
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

/// An image's encoding, made once per key and format (files share images): `cache/<key>.bc1|bc2` holds w, h, data.
fn encoded(cache: &Path, key: &str, png: &Path, dxt1: bool) -> Result<(u32, u32, Vec<u8>)> {
    let path = cache.join(format!("{key}.{}", if dxt1 { "bc1" } else { "bc2" }));
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
    let data = encode(dxt1, w, h, &rgba);
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
/// name suffix allowed) into `out/ROM*/dir/file.DAT`. Files already built are kept and every encoding is cached in
/// `<manifest>/encoded`, so it can resume.
pub fn pack(manifest_dir: &Path, up: &Path, out: &Path) -> Result<()> {
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
                    if dest.exists() {
                        continue;
                    }
                    let Ok(bytes) = std::fs::read(&stock) else { continue };
                    let Ok(chunks) = dat::chunks(&bytes) else { continue };
                    let keys: HashMap<usize, &String> = wanted.iter().map(|(k, key)| (*k, key)).collect();
                    let mut outb = Vec::with_capacity(bytes.len() * 8);
                    let mut changed = false;
                    for (k, c) in chunks.iter().enumerate() {
                        let header = &bytes[c.offset..c.offset + 16];
                        let end = c.offset + 16 + c.data.len();
                        let up = keys.get(&k).and_then(|key| by_key.get(*key).map(|p| (key, p)));
                        match up.map(|(key, p)| encoded(&cache, key, p, is_dxt1(c.data))) {
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
        assert!(t.rgba.chunks(4).all(|px| px[3] >= 254));
        let header = [b'i', b'm', b'g', b'1', 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let c = chunk_bytes(&header, &p);
        assert_eq!(c.len() % 16, 0);
        let parsed = dat::chunks(&c).unwrap();
        assert_eq!((parsed[0].kind, parsed[0].data.len() >= p.len()), (0x20, true));
    }
}
