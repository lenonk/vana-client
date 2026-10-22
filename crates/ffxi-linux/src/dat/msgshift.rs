//! LandSandBoat text ids -> installed zone dialogue ids (s2c 0x036 / 0x02A index the zone dialogue
//! file, FUN_10094d20(8, id), and LSB's ids come from a newer client). The bundled table holds only
//! the derived segments, keyed by each zone dialogue file's md5 so another install is not mis-mapped.
//! `ffxi-linux msgshift-gen` regenerates it by matching LSB IDs.lua comments against the DAT text.

use super::DatIndex;
use super::dialog::{Dialog, file_id};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::Path;

const BUNDLED: &str = include_str!("../../data/msgshift.txt");

/// `from..=to` (LSB ids) map to `id + delta`; `anchors` = matched texts inside.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seg {
    pub from: u32,
    pub to: u32,
    pub delta: i32,
    pub anchors: u32,
}

#[derive(Clone, Debug)]
pub struct ZoneShift {
    /// md5 of the dialogue file the mapping was made from (first 8 bytes, hex).
    pub md5: String,
    pub confidence: String,
    pub segs: Vec<Seg>,
}

#[derive(Clone, Debug, Default)]
pub struct MsgShift {
    pub zones: BTreeMap<u16, ZoneShift>,
}

impl MsgShift {
    pub fn bundled() -> MsgShift {
        Self::parse(BUNDLED)
    }

    pub fn parse(text: &str) -> MsgShift {
        let mut zones = BTreeMap::new();
        let mut cur: Option<u16> = None;
        for l in text.lines() {
            let f: Vec<&str> = l.split_whitespace().collect();
            match f.as_slice() {
                ["zone", z, md5, conf] => {
                    let Ok(z) = z.parse() else { continue };
                    zones.insert(z, ZoneShift { md5: md5.to_string(), confidence: conf.to_string(), segs: Vec::new() });
                    cur = Some(z);
                }
                [a, b, d, n] if !a.starts_with('#') => {
                    let (Some(z), Ok(from), Ok(to), Ok(delta), Ok(anchors)) = (cur, a.parse(), b.parse(), d.parse(), n.parse()) else {
                        continue;
                    };
                    zones.get_mut(&z).unwrap().segs.push(Seg { from, to, delta, anchors });
                }
                _ => {}
            }
        }
        MsgShift { zones }
    }

    /// Keeps only zones whose dialogue file matches the install.
    pub fn verified(mut self, index: &DatIndex) -> MsgShift {
        self.zones.retain(|&z, s| index.read(file_id(z)).is_ok_and(|d| md5_8(&d) == s.md5));
        self
    }

    /// Installed message id for an LSB id, or None outside the matched segments.
    pub fn map(&self, zone: u16, lsb: u32) -> Option<u32> {
        let s = self.zones.get(&zone)?.segs.iter().find(|s| s.from <= lsb && lsb <= s.to)?;
        lsb.checked_add_signed(s.delta)
    }
}

fn md5_8(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    hex::encode(&Md5::digest(data)[..8])
}

/// Dialogue text with control codes blanked (lengths as dialog::readable, FUN_100bf790).
fn dat_text(s: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        let a = s.get(i + 1).copied().unwrap_or(0);
        if c >= 0x80 {
            i += 2;
        } else if (0x20..0x7f).contains(&c) {
            out.push(c as char);
            i += 1;
            continue;
        } else if c == 0 {
            break;
        } else {
            i += match c {
                0x01 => 2 + a as usize,
                0x02 | 0x03 => 3,
                0x05 | 0x0a | 0x0c | 0x0d | 0x0e | 0x10..=0x14 | 0x16..=0x1a | 0x1c | 0x1e | 0x1f => 2,
                0x7f => match a {
                    0x38 => 4,
                    0x34..=0x36 | 0x80 | 0x81 | 0x84 | 0x86..=0x88 | 0x8f | 0x92 | 0x94..=0x97 | 0xa0..=0xac | 0xb0 | 0xb1 | 0xb4 | 0xb5 => 3,
                    _ => 2,
                },
                _ => 1,
            };
        }
        out.push(' ');
    }
    out
}

/// Lower-case letters only, with [a/b] alternatives and <placeholders> removed.
fn norm(t: &str) -> String {
    let mut out = String::new();
    let mut skip: Option<char> = None;
    for c in t.chars() {
        match (skip, c) {
            (None, '[') => skip = Some(']'),
            (None, '<') => skip = Some('>'),
            (Some(e), _) if c == e => skip = None,
            (Some(_), _) => {}
            (None, _) if c.is_ascii_alphabetic() => out.push(c.to_ascii_lowercase()),
            _ => {}
        }
    }
    out
}

/// `NAME = 123,` lines of scripts/enum/zone.codegen.lua.
fn zone_ids(lsb: &Path) -> Result<BTreeMap<String, u16>> {
    let text = std::fs::read_to_string(lsb.join("scripts/enum/zone.codegen.lua")).context("zone.codegen.lua")?;
    let mut m = BTreeMap::new();
    for l in text.lines() {
        if let Some((k, v)) = l.trim().split_once('=') {
            let k = k.trim();
            let v = v.trim().trim_end_matches(',').trim();
            if !k.is_empty() && k.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
                if let Ok(v) = v.parse() {
                    m.insert(k.to_string(), v);
                }
            }
        }
    }
    Ok(m)
}

/// (zone name, [(id, comment)]) from the `text = { NAME = id, -- comment }` table of an IDs.lua.
fn lsb_entries(text: &str) -> (Option<String>, Vec<(u32, String)>) {
    let (mut zone, mut out, mut depth, mut intext) = (None, Vec::new(), 0i32, false);
    for l in text.lines() {
        if let Some(r) = l.trim_start().strip_prefix("zones[xi.zone.") {
            zone = r.split(']').next().map(|s| s.to_string());
        }
        let t = l.trim();
        if t == "text =" || t.starts_with("text = {") || t.starts_with("text={") {
            intext = true;
            depth = 0;
        }
        if !intext {
            continue;
        }
        depth += l.matches('{').count() as i32 - l.matches('}').count() as i32;
        if let Some((lhs, rest)) = t.split_once('=') {
            if let Some((num, com)) = rest.split_once(',') {
                let com = com.trim();
                if let (Ok(id), Some(c)) = (num.trim().parse::<u32>(), com.strip_prefix("--")) {
                    if lhs.trim().chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
                        out.push((id, c.trim().to_string()));
                    }
                }
            }
        }
        if depth <= 0 && l.contains('}') {
            intext = false;
        }
    }
    (zone, out)
}

struct Analysis {
    commented: usize,
    matched: usize,
    anchors: usize,
    outliers: usize,
    inside_bad: usize,
    amb_bad: usize,
    segs: Vec<Seg>,
}

/// Unique text matches become anchors (delta = dat id - lsb id); equal neighbours merge into segments.
fn analyse(entries: &[(u32, String)], dialog: &Dialog) -> Analysis {
    let nm: Vec<String> = (0..dialog.len() as u32).map(|i| norm(&dat_text(&dialog.raw(i).unwrap_or_default()))).collect();
    let mut by: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
    for (i, t) in nm.iter().enumerate() {
        if !t.is_empty() {
            by.entry(t.as_str()).or_default().push(i as u32);
        }
    }
    // (lsb id, usable anchor, candidates)
    let mut rows: Vec<(u32, bool, Vec<u32>)> = Vec::new();
    let mut commented = 0;
    for (lid, com) in entries {
        let t = norm(com);
        if t.is_empty() {
            continue;
        }
        commented += 1;
        let mut c = by.get(t.as_str()).cloned().unwrap_or_default();
        if c.is_empty() && t.len() >= 20 {
            c = nm.iter().enumerate().filter(|(_, x)| x.starts_with(&t) || (x.len() >= 20 && t.starts_with(x.as_str()))).map(|(i, _)| i as u32).collect();
        }
        let short = t.len() < 8 && c.len() > 1;
        rows.push((*lid, !short, c));
    }
    let mut anchors: Vec<(u32, i32)> =
        rows.iter().filter(|r| r.1 && r.2.len() == 1).map(|r| (r.0, r.2[0] as i32 - r.0 as i32)).collect();
    anchors.sort();
    // A lone delta between two neighbours that agree is an outlier.
    let out: Vec<bool> = (0..anchors.len())
        .map(|i| i > 0 && i + 1 < anchors.len() && anchors[i - 1].1 == anchors[i + 1].1 && anchors[i].1 != anchors[i - 1].1)
        .collect();
    let mut segs: Vec<Seg> = Vec::new();
    for (i, &(lid, d)) in anchors.iter().enumerate() {
        if out[i] {
            continue;
        }
        match segs.last_mut() {
            Some(s) if s.delta == d => {
                s.to = lid;
                s.anchors += 1;
            }
            _ => segs.push(Seg { from: lid, to: lid, delta: d, anchors: 1 }),
        }
    }
    let seg_of = |lid: u32| segs.iter().find(|s| s.from <= lid && lid <= s.to);
    let mut inside_bad = 0;
    let mut amb_bad = 0;
    for (lid, usable, c) in &rows {
        let Some(s) = seg_of(*lid) else { continue };
        let at = (*lid as i64 + s.delta as i64) as u32;
        if c.len() > 1 || !usable {
            if !c.is_empty() && !c.contains(&at) {
                amb_bad += 1;
            }
        } else if !c.is_empty() && !c.contains(&at) {
            inside_bad += 1;
        }
    }
    Analysis {
        commented,
        matched: rows.iter().filter(|r| !r.2.is_empty()).count(),
        anchors: anchors.len() - out.iter().filter(|&&o| o).count(),
        outliers: out.iter().filter(|&&o| o).count(),
        inside_bad,
        amb_bad,
        segs,
    }
}

fn confidence(a: &Analysis) -> &'static str {
    if a.anchors < 3 {
        return "low";
    }
    let frac = a.matched as f32 / a.commented.max(1) as f32;
    let clean = a.outliers == 0 && a.inside_bad == 0 && a.amb_bad == 0;
    let multi = a.segs.iter().filter(|s| s.anchors >= 2).map(|s| s.anchors).sum::<u32>() as f32 / a.anchors.max(1) as f32;
    if frac >= 0.9 && clean && multi >= 0.8 {
        "high"
    } else if frac >= 0.6 && a.inside_bad <= 1 {
        "medium"
    } else {
        "low"
    }
}

/// `msgshift-gen`: matches every zone of an LSB checkout against the install and writes the table.
pub fn generate(index: &DatIndex, lsb: &Path, out: &Path) -> Result<()> {
    let ids = zone_ids(lsb)?;
    let mut text = String::from(
        "# LandSandBoat text id -> installed zone dialogue id, from `ffxi-linux msgshift-gen`.\n\
         # zone ZONE MD5 CONFIDENCE (md5 = first 8 bytes of the dialogue file it was made from)\n\
         # FROM TO DELTA ANCHORS: LSB ids FROM..=TO map to id + DELTA; ids between segments are unknown.\n",
    );
    let (mut zones, mut commented, mut matched) = (0, 0, 0);
    let mut dirs: Vec<_> = std::fs::read_dir(lsb.join("scripts/zones"))?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    dirs.sort();
    let mut rows = Vec::new();
    for d in dirs {
        let Ok(lua) = std::fs::read_to_string(d.join("IDs.lua")) else { continue };
        let (Some(name), entries) = lsb_entries(&lua) else { continue };
        let Some(&z) = ids.get(&name) else { continue };
        if entries.is_empty() {
            continue;
        }
        let Ok(raw) = index.read(file_id(z)) else { continue };
        let Ok(dialog) = Dialog::parse(&raw) else { continue };
        let a = analyse(&entries, &dialog);
        zones += 1;
        commented += a.commented;
        matched += a.matched;
        rows.push((z, md5_8(&raw), confidence(&a), a.segs));
    }
    rows.sort_by_key(|r| r.0);
    for (z, md5, conf, segs) in rows {
        text += &format!("zone {z} {md5} {conf}\n");
        for s in segs {
            text += &format!("{} {} {} {}\n", s.from, s.to, s.delta, s.anchors);
        }
    }
    std::fs::write(out, &text)?;
    println!("{zones} zones, {matched} of {commented} commented entries matched; wrote {}", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_map() {
        let s = MsgShift::parse("# c\nzone 230 0011223344556677 high\n6818 8244 -43 63\n8263 10355 -44 29\n");
        assert_eq!(s.map(230, 8002), Some(7959));
        assert_eq!(s.map(230, 8250), None);
        assert_eq!(s.map(230, 8263), Some(8219));
        assert_eq!(s.map(231, 8002), None);
    }

    #[test]
    fn normalise() {
        assert_eq!(norm("You obtain <item>, [sir/madam]!"), "youobtain");
        assert_eq!(dat_text(b"Hi \x01\x05\x33\x82\x80\x80\x80 there\x7f\x31"), "Hi   there ");
    }

    #[test]
    fn bundled_zone_230() {
        let s = MsgShift::bundled();
        assert_eq!(s.map(230, 8002), Some(7959));
    }
}
