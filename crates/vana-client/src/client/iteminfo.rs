//! The item info window ("menu    iteminfo", CTkItemInfo): strings FUN_1013d790, rows FUN_1013c500 (en / de / fr),
//! draw FUN_1013d760. Opened by event ops 0x93 / 0xcc and, for the item under the cursor, by the item lists.

use super::game_ui::GameUi;
use crate::dat::items::Item;

/// Layouts by row count 3..12 (table 0x1036fa70): 366 x (8 + 16 rows).
/// itemxinf's layouts by rows, 3..12 (the compare window above itemyinf).
pub const XLAYOUTS: [&str; 10] = [
    "menu    itemxinf",
    "menu    itemx4in",
    "menu    itemx5in",
    "menu    itemx6in",
    "menu    itemx7in",
    "menu    itemx8in",
    "menu    itemx9in",
    "menu    itemx10i",
    "menu    itemx11i",
    "menu    itemx12i",
];

pub const LAYOUTS: [&str; 10] = [
    "menu    iteminfo",
    "menu    item4inf",
    "menu    item5inf",
    "menu    item6inf",
    "menu    item7inf",
    "menu    item8inf",
    "menu    item9inf",
    "menu    item10in",
    "menu    item11in",
    "menu    item12in",
];

const WHITE: u32 = 0x8080_8080;
/// The page arrow "▼" (SJIS 81 A5).
const MORE: [u8; 2] = [0x81, 0xa5];

/// The game around the item.
#[derive(Clone, Debug, Default)]
pub struct Ctx {
    /// Vana'diel-epoch seconds (FUN_101039e0), for charge timers.
    pub now: u32,
    /// The slot is equipped (FUN_10131f10): a green name.
    pub equipped: bool,
    /// Shop style (+0x370): a yellow name.
    pub shop: bool,
    /// The container (+0x340; 3 is temporary items).
    pub container: Option<u8>,
    /// +0x374: bit 0 hides the extra line, bit 1 the signature.
    pub flags: u32,
    /// A Maze Tabula's voucher by name (its compact record, FUN_1015bbd0), when its extdata names one.
    pub voucher: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    pub name: String,
    pub name_tint: u32,
    pub icon_tint: u32,
    /// windowps marks left to right (293 Aug, 294 Rare, 295 Ex, 296 Tmp) and the Alt mark (842) before them.
    pub marks: Vec<usize>,
    pub alt: bool,
    /// The header's second line (skill or slot, races, superior level), drawn part after part.
    pub line2: Vec<String>,
    pub desc: Vec<String>,
    pub jobs: Vec<String>,
    pub augs: Vec<String>,
    /// +0x2c (charges, fish, linkshell ...) and its colour, +0xac (signature), +0x278 (item level).
    pub extra: String,
    pub extra_tint: u32,
    pub sig: String,
    pub ilvl: String,
    /// Row codes per page (0 blank, 1 header, 10+i description, 0x1e+i augments, 0x32+i jobs, 0x3c..0x3e the lines,
    /// +10 with the arrow, 0x50 the arrow alone).
    pub pages: Vec<Vec<u8>>,
}

/// C-style "%d" / "%s" substitution in order.
fn fmt(f: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::new();
    let mut it = args.iter();
    let mut chars = f.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' && matches!(chars.peek(), Some('d' | 's')) {
            chars.next();
            if let Some(a) = it.next() {
                out.push_str(&a.to_string());
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// FUN_101c93c0: "%d:%02d:%02d" from an hour up, else "%d:%02d".
fn time(s: u32) -> String {
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// Greedy wrap at `max` px, breaking after any of `breaks` (FUN_101ca520); a piece with no break point is cut.
fn wrap(s: &str, max: f32, breaks: &[char], width: &dyn Fn(&str) -> f32) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s.to_string();
    while width(&rest) > max {
        let mut cut = 0;
        let mut last_break = None;
        for (i, c) in rest.char_indices() {
            let end = i + c.len_utf8();
            if width(&rest[..end]) > max {
                break;
            }
            cut = end;
            if breaks.contains(&c) {
                last_break = Some(end);
            }
        }
        let at = last_break.unwrap_or(cut.max(rest.chars().next().map_or(1, char::len_utf8)));
        out.push(rest[..at].trim_end().to_string());
        rest = rest[at..].trim_start().to_string();
    }
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

/// FUN_100fe150: 6-bit MSB-first names (1..10 '0'..'9', 11..36 'A'..'Z', 37..62 'a'..'z', 0 ends).
fn sig_name(b: &[u8]) -> String {
    let mut out = String::new();
    for k in 0..b.len() * 8 / 6 {
        let bit = k * 6;
        let v = ((u16::from(b[bit / 8]) << 8 | u16::from(*b.get(bit / 8 + 1).unwrap_or(&0))) >> (10 - bit % 8)) & 0x3f;
        out.push(match v {
            0 => break,
            1..=10 => (b'0' + v as u8 - 1) as char,
            11..=36 => (b'A' + v as u8 - 11) as char,
            37..=62 => (b'a' + v as u8 - 37) as char,
            _ => '?',
        });
    }
    out
}

/// Group 12 ids of the weapon skills (FUN_1015aab0: key skill + 1).
fn skill_text(skill: u8) -> Option<usize> {
    Some(match skill {
        1..=12 => 0x17 + skill as usize,
        25 => 0x24,
        26 => 0x25,
        27 => 0x26,
        41 => 0x27,
        42 => 0x28,
        45 => 0x29,
        48 => 0x2a,
        _ => return None,
    })
}

/// FUN_1013cec0's line 2 tables: slots (group 12 #0xe..0x17, first match) and races (#0..0xd: the first three exact,
/// then the first whose bits are all set *(inf)*).
const SLOTS: [(u16, usize); 10] = [(0x10, 0xe), (0x20, 0xf), (0x80, 0x10), (0x40, 0x11), (0x2000, 0x12), (0x800, 0x13), (0x200, 0x14), (0x8000, 0x15), (0x400, 0x16), (0x100, 0x17)];
const RACES: [u16; 14] = [0x1fe, 0x12a, 0xd4, 6, 2, 4, 0x18, 8, 0x10, 0x60, 0x20, 0x40, 0x100, 0x80];

/// FUN_1013d790 and FUN_1013c500. `msg(group, id)`: groups 9..12 and 0x1c (file slot 0x1c, 55688 in English);
/// `width` the game font's.
pub fn build(it: &Item, ext: Option<&[u8; 24]>, cx: &Ctx, msg: &dyn Fn(u8, usize) -> Option<String>, width: &dyn Fn(&str) -> f32, aug_ctx: Option<&super::augment::AugCtx>) -> Info {
    let mut info = Info { name_tint: WHITE, icon_tint: WHITE, extra_tint: WHITE, ..Default::default() };
    let mut name = it.singular.clone();
    if let Some(c) = name.get(..1) {
        name.replace_range(..1, &c.to_uppercase());
    }
    info.name = name;
    info.name_tint = if cx.shop {
        0x8090_9040
    } else if cx.equipped {
        0x8050_9050
    } else {
        WHITE
    };
    let t = it.item_type;
    let gear = matches!(t, 4 | 5);
    // FUN_1013cc50 with FUN_101cb8a0 / FUN_101cb990: Aug for weapons and armour with augment extdata (not the no-mark
    // weapons nor a serialized item); its flag byte & 2 adds the Ex mark.
    let fb = aug_ctx.and_then(|a| super::augment::marked(it.id as u16, t, ext, a.tables));
    let aug = fb.is_some();
    if aug {
        info.marks.push(293);
    }
    if it.flags & 0x8000 != 0 {
        info.marks.push(294);
    }
    if it.flags & 0x4000 != 0 || fb.is_some_and(|f| f & 2 != 0) {
        info.marks.push(295);
    }
    if cx.container == Some(3) {
        info.marks.push(296);
    }
    info.alt = it.flags & 0x10 != 0 && !(it.flags & 0x800 != 0 && aug && it.flags & 1 == 0);
    if gear {
        let first = if t == 4 { skill_text(it.skill) } else { SLOTS.iter().find(|(b, _)| it.slots & b != 0).map(|s| s.1) };
        if let Some(s) = first.and_then(|id| msg(12, id)) {
            info.line2.push(s);
        }
        let race = RACES.iter().position(|&r| it.races == r).filter(|&k| k < 3).or_else(|| RACES.iter().skip(3).position(|&r| it.races & r == r).map(|k| k + 3));
        if let Some(s) = race.and_then(|k| msg(12, k)) {
            info.line2.push(s);
        }
        if it.superior_level != 0
            && let Some(f) = msg(12, 0x1bc)
        {
            info.line2.push(fmt(&f, &[&it.superior_level]));
        }
    }
    // Description: the item's lines, pre-wrapped at 316 px (FUN_1015b410), at most 20.
    info.desc = it.description.split('\n').flat_map(|l| wrap(l, 316.0, &[' '], width)).take(20).collect();
    let extra = |k: u16| it.extra.iter().find(|e| e.0 == k).map_or(0, |e| e.1);
    let mut line = String::new();
    if gear {
        // FUN_1013ae30: "Lv.%d " and the jobs, or "All Jobs"; none for fishing or skill-less weapons.
        let skilled = !(t == 4 && matches!(it.skill, 0xff | 0x30));
        if it.jobs & 0x7f_fffe != 0 && skilled {
            let jobs = if it.jobs & 0x7f_fffe == 0x7f_fffe {
                msg(9, 0xe2).unwrap_or_default()
            } else {
                (1..super::items::JOBS.len()).filter(|j| it.jobs & (1 << j) != 0).map(|j| super::items::JOBS[j]).collect::<Vec<_>>().join("/")
            };
            let lv = fmt(&msg(10, 0x53).unwrap_or_else(|| "Lv.%d ".into()), &[&it.level]);
            info.jobs = super::augment::wrap(&format!("{lv}{jobs}"), 309.0, width);
        }
        let ilvl = extra(0xd50);
        if ilvl != 0 {
            info.ilvl = format!("<{}>", fmt(&msg(0x1c, 0x3e).unwrap_or_else(|| "%d".into()), &[&ilvl]));
        }
        // FUN_1013bbf0: charges (extdata kind none / 1); augments (2 / 3) are not decoded yet.
        // Charges only with no extdata (an event's item) or kind 1.
        if it.max_charges != 0 && ext.is_none_or(|e| e[0] == 1) {
            let u32_at = |e: &[u8; 24], o: usize| u32::from_le_bytes(e[o..o + 4].try_into().unwrap());
            let left = |ts: u32| ts.saturating_sub(cx.now);
            let mut tt = ext.map_or(0, |e| left(u32_at(e, 4)));
            match ext {
                Some(e) if cx.equipped && cx.container.is_some_and(|c| matches!(c, 0 | 8 | 10..=16)) => tt = tt.max(left(u32_at(e, 8))),
                _ => tt = tt.max(it.use_delay as u32),
            }
            let max = it.max_charges;
            let cur = ext.map_or(max, |e| e[1]);
            if cur == 0 && ext.is_some() {
                tt = 0;
            }
            line = if max == 0xff { crate::dat::font::decode_sjis(&[0x81, 0xe4]) } else { format!("{cur}/{max}") };
            if cur == 0 {
                info.extra_tint = 0x8080_4020;
            } else if cur == max {
                info.extra_tint = 0x8040_7090;
            }
            line += &format!(" {}/[{}, {}]", time(tt), time(it.use_delay as u32), time(it.reuse_delay));
        }
    }
    // Augments (FUN_1013b170 -> FUN_100ff140, FUN_1013b280 modes 0 / 1; a stone, type 0x1a, modes 2 / 3): the block
    // wrapped at 309 px, the short line into "<...>".
    if let (Some(a), Some(e)) = (aug_ctx, ext)
        && ((gear && super::augment::applies(it.id as u16, t, e)) || t == 0x1a)
    {
        let r = super::augment::decode(e, it.id as u16, a);
        let stone = t == 0x1a;
        if !stone || !r.pairs.is_empty() {
            let block = crate::dat::font::decode_sjis(&super::augment::block(&r, stone, a));
            info.augs = super::augment::wrap(&block, 309.0, width).into_iter().take(20).collect();
            if let Some((s, tint)) = super::augment::short(&r, stone, a) {
                line = crate::dat::font::decode_sjis(&s);
                if let Some(c) = tint {
                    info.extra_tint = c;
                }
            }
        }
    }
    match (t, ext) {
        // Fish: size and weight.
        (3, Some(e)) if u16::from_le_bytes([e[0], e[1]]) != 0 => {
            line = format!("{}[Im]/{}[Pz]", u16::from_le_bytes([e[0], e[1]]), u16::from_le_bytes([e[2], e[3]]));
            if e[4] & 1 != 0
                && let Some(s) = msg(11, 0xb7)
            {
                line += &format!(" ({s})");
            }
        }
        // Linkshell (FUN_101cbac0): name and ARGB tint, R = [6] low, G = [6] high, B = [7] low nibble, each x 13 + 0x38;
        // a grey line when broken (status 4).
        (6, Some(e)) if e[8] != 0 => {
            line = super::linkshell::ls_name(&e[9..]);
            let c = |n: u8| (n & 0xf) as u32 * 13 + 0x38;
            info.icon_tint = 0x8000_0000 | c(e[6]) << 16 | c(e[6] >> 4) << 8 | c(e[7]);
            if e[8] == 4 {
                info.extra_tint = 0x8040_4040;
            }
        }
        // Maze Tabulae (cases 0x16 / 0x17): the voucher and the runes on it, "%s [%d]"; an R one adds d5 bits 4..10, "(%d)".
        (0x16 | 0x17, Some(e)) if cx.voucher.is_some() => {
            let (name, n) = (cx.voucher.clone().unwrap_or_default(), crate::dat::maze::count(e));
            line = if t == 0x17 { format!("{name} [{n}]({})", crate::dat::maze::extra(e)) } else { format!("{name} [{n}]") };
        }
        // Furnishings: the element glyph and the storage.
        (10 | 12 | 14, _) => {
            let el = extra(0xd10);
            let glyph = if el <= 7 { crate::dat::font::decode_sjis(&[0xef, 0x1f + el as u8]) } else { String::new() };
            if let Some(f) = msg(10, 0x38) {
                line = fmt(&f, &[&glyph, &extra(0xd14)]);
            }
        }
        _ => {}
    }
    if !line.is_empty() {
        info.extra = format!("<{line}>");
    }
    // The crafter's signature.
    let sig_types = matches!(t, 0 | 1 | 2 | 4 | 5 | 7 | 0x10 | 0x1c | 0x1f);
    let off = if t == 0x10 { 0xa } else { 0xc };
    if let Some(e) = ext.filter(|e| sig_types && e[off] != 0 && !matches!(it.id, 0x200..=0x203 | 0x108c | 0x108d | 0x1525 | 0x1526)) {
        info.sig = format!("[{}]", sig_name(&e[off..]));
    }
    if cx.flags & 1 != 0 {
        info.extra.clear();
    }
    if cx.flags & 2 != 0 {
        info.sig.clear();
    }
    info.pages = pages(&info, gear, cx.container == Some(3) || aug, it.flags);
    info
}

/// FUN_1013c500: the row codes of up to three pages.
fn pages(info: &Info, two_rows: bool, _plus: bool, _flags: u16) -> Vec<Vec<u8>> {
    let head: Vec<u8> = if two_rows { vec![1, 0] } else { vec![1] };
    let n = head.len();
    let min_rows = 3;
    let (d, j, a) = (info.desc.len(), info.jobs.len(), info.augs.len());
    let lines: Vec<(bool, u8)> = [(!info.extra.is_empty(), 0x3c), (!info.sig.is_empty(), 0x3d), (!info.ilvl.is_empty(), 0x3e)].to_vec();
    let used = lines.iter().filter(|l| l.0).count();
    let mut avail = 12usize.saturating_sub(used + j + n);
    let (mut alone, mut tail) = (false, false);
    if avail < d || a > 0 {
        if used == 0 {
            avail = avail.saturating_sub(1);
            alone = true;
        } else {
            tail = true;
        }
    }
    let pad = |c: &mut Vec<u8>| {
        while c.len() < min_rows {
            c.push(0);
        }
    };
    let mut p0 = head.clone();
    let shown = d.min(avail);
    p0.extend((0..shown).map(|i| 10 + i as u8));
    p0.extend((0..j).map(|i| 0x32 + i as u8));
    p0.extend(lines.iter().filter(|l| l.0).map(|l| l.1));
    pad(&mut p0);
    if tail && matches!(p0.last(), Some(0x3c..=0x3e)) {
        *p0.last_mut().unwrap() += 10;
    }
    if alone {
        if p0.last() == Some(&0) {
            *p0.last_mut().unwrap() = 0x50;
        } else {
            p0.push(0x50);
        }
    }
    let mut out = vec![p0];
    let close = |c: &mut Vec<u8>| {
        pad(c);
        // The item level line with its arrow is always appended; the arrow alone takes a blank last row.
        if !info.ilvl.is_empty() {
            c.push(0x48);
        } else if c.last() == Some(&0) {
            *c.last_mut().unwrap() = 0x50;
        } else {
            c.push(0x50);
        }
    };
    // Page 1: the rest of the description (to row 10), else the augment lines (to row 10, the rest dropped); page 2
    // only after a description page, with every augment line.
    if shown < d {
        let mut p = head.clone();
        p.extend((shown..d).map(|i| 10 + i as u8).take(11 - head.len()));
        close(&mut p);
        out.push(p);
        if a > 0 {
            let mut p = head.clone();
            p.extend((0..a).map(|i| 0x1e + i as u8));
            close(&mut p);
            out.push(p);
        }
    } else if a > 0 {
        let mut p = head.clone();
        p.extend((0..a).map(|i| 0x1e + i as u8).take(11 - head.len()));
        close(&mut p);
        out.push(p);
    }
    out
}

/// Rows of a page: its codes, at least 3, at most 12.
pub fn rows(info: &Info, page: usize) -> usize {
    info.pages.get(page).map_or(3, Vec::len).clamp(3, 12)
}

/// FUN_1013d760 at frame origin (x, y): the header, then each row's line at y + 4 + 16 k.
pub fn draw(gu: &mut GameUi, info: &Info, page: usize, icon: Option<&str>, x: f32, y: f32) {
    let col = |c: u32| c.to_le_bytes();
    let more = crate::dat::font::decode_sjis(&MORE);
    let right = |gu: &mut GameUi, s: &str, rx: f32, ry: f32, c: u32| {
        let w = gu.text_width(s);
        gu.text(s, rx - w, ry, 0, col(c));
    };
    let Some(codes) = info.pages.get(page) else { return };
    for (k, &c) in codes.iter().enumerate() {
        let ry = y + 4.0 + 16.0 * k as f32;
        match c {
            1 => {
                if let Some(t) = icon {
                    let tint = info.icon_tint.to_le_bytes().map(|b| b as f32 / 128.0);
                    gu.image_rect(t, x + 8.0, y + 12.0, 32.0, 32.0, [tint[2], tint[1], tint[0], tint[3]]);
                }
                gu.text(&info.name, x + 51.0, y + 4.0, 0, col(info.name_tint));
                let mut mx = x + 314.0;
                if info.alt {
                    gu.sprite("menu    windowps", 842, mx - 16.0, y + 4.0, WHITE);
                }
                for &m in &info.marks {
                    gu.sprite("menu    windowps", m, mx, y + 4.0, WHITE);
                    mx += 16.0;
                }
                let mut lx = x + 51.0;
                for part in &info.line2 {
                    gu.text(part, lx, y + 20.0, 0, col(WHITE));
                    lx += gu.text_width(part);
                }
            }
            10..=29 => {
                if let Some(l) = info.desc.get(c as usize - 10) {
                    gu.text(l, x + 51.0, ry, 0, col(WHITE));
                }
            }
            0x1e..=0x31 => {
                if let Some(l) = info.augs.get(c as usize - 0x1e) {
                    gu.text(l.trim_end_matches('\n'), x + 51.0, ry, 0, col(0x8080_8050));
                }
            }
            0x32..=0x3b => {
                if let Some(l) = info.jobs.get(c as usize - 0x32) {
                    gu.text(l, x + 51.0, ry, 0, col(WHITE));
                }
            }
            0x3c..=0x3e | 0x46..=0x48 => {
                let (s, tint) = match c % 10 {
                    0 => (&info.extra, info.extra_tint),
                    1 => (&info.sig, WHITE),
                    _ => (&info.ilvl, WHITE),
                };
                if c >= 0x46 {
                    right(gu, s, x + 340.0, ry, tint);
                    right(gu, &more, x + 360.0, ry, WHITE);
                } else {
                    right(gu, s, x + 360.0, ry, tint);
                }
            }
            0x50 => right(gu, &more, x + 360.0, ry, WHITE),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u32) -> Option<Item> {
        let dir = crate::config::Config::load().ffxi_dir();
        let index = crate::dat::DatIndex::open(&dir).ok()?;
        crate::dat::items::Items::load(&index, crate::dat::items::Lang::En).ok()?.item(id)
    }

    /// 7 px a character, about the game font's.
    fn width(s: &str) -> f32 {
        s.chars().count() as f32 * 7.0
    }

    fn msg(g: u8, id: usize) -> Option<String> {
        Some(match (g, id) {
            (9, 0xe2) => "All Jobs".into(),
            (10, 0x53) => "Lv.%d ".into(),
            (12, 0) => "All Races ".into(),
            (12, 0x1a) => "(Sword)".into(),
            _ => return None,
        })
    }

    #[test]
    fn bronze_sword() {
        let Some(it) = item(0x4097) else { return };
        let i = build(&it, None, &Ctx::default(), &msg, &width, None);
        assert_eq!(i.name, "Bronze sword");
        assert_eq!(i.line2, vec!["(Sword)".to_string(), "All Races ".into()]);
        assert_eq!(i.jobs.concat().replace(' ', ""), "Lv.1WAR/RDM/THF/PLD/DRK/BRD/RNG/NIN/DRG/BLU/RUN");
        assert_eq!(&i.pages[0][..3], &[1, 0, 10]);
        assert_eq!(i.pages.len(), 1);
    }

    #[test]
    fn kupofried_ring() {
        let Some(it) = item(0x3de0) else { return };
        let i = build(&it, None, &Ctx::default(), &msg, &width, None);
        assert_eq!(i.marks, vec![294, 295]);
        assert_eq!((i.extra.as_str(), i.extra_tint), ("<11/11 0:05/[0:05, 15:00]>", 0x8040_7090));
        assert_eq!(i.pages[0], vec![1, 0, 10, 11, 12, 0x32, 0x3c]);
    }

    #[test]
    fn long_description_pages() {
        let mut it = Item::default();
        it.item_type = 1;
        it.description = (0..14).map(|k| format!("line {k}")).collect::<Vec<_>>().join("\n");
        let i = build(&it, None, &Ctx::default(), &msg, &width, None);
        assert_eq!(i.pages[0].len(), 12);
        assert_eq!(*i.pages[0].last().unwrap(), 0x50);
        assert_eq!(i.pages[1][0], 1);
        assert!(i.pages[1].contains(&(10 + 10)));
    }

    #[test]
    fn linkshell_and_signature() {
        let mut e = [0u8; 24];
        (e[6], e[7], e[8]) = (0x2f, 0x01, 1);
        // "Ab": 0x1b ('A'), 2 ('b'), 0x3f (end), MSB first.
        let bits: u32 = (0x1b << 18) | (2 << 12) | (0x3f << 6);
        e[9..12].copy_from_slice(&bits.to_be_bytes()[1..]);
        let mut it = Item::default();
        it.item_type = 6;
        let i = build(&it, Some(&e), &Ctx::default(), &msg, &width, None);
        assert_eq!(i.extra, "<Ab>");
        assert_eq!(i.icon_tint, 0x8000_0000 | (15 * 13 + 0x38) << 16 | (2 * 13 + 0x38) << 8 | (13 + 0x38));
        let mut s = [0u8; 24];
        // "A1": 11, 2.
        let bits: u32 = (11 << 18) | (2 << 12);
        s[0xc..0xf].copy_from_slice(&bits.to_be_bytes()[1..]);
        let mut it = Item::default();
        (it.item_type, it.id) = (1, 1);
        assert_eq!(build(&it, Some(&s), &Ctx::default(), &msg, &width, None).sig, "[A1]");
    }
}
