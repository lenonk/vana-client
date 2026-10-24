//! The client message formatter (FUN_10153310 via FUN_10153150 / FUN_10153180; spec
//! ~/ffxi/lab/research/msgfmt_spec.md): group 8 / 10 strings carry `FA 40 <code>` escapes for numbers, string
//! arguments, choices, compares and jumps, and item names. Plus the date / time builders FUN_101c8fd0 (Earth) and
//! FUN_101c92b0 (Vana'diel) over the group 10 format rows.

/// FUN_101531d0: `01 n` = ints[n - 1], `02 n` = n, `03 lo hi` = a 14-bit literal; anything else reads nothing.
fn arg(s: &[u8], p: usize, ints: &[i32]) -> (i32, usize) {
    let b = |k: usize| s.get(p + k).copied().unwrap_or(0);
    match b(0) {
        1 => (ints.get(((b(1) & 0x7f) as usize).wrapping_sub(1)).copied().unwrap_or(0), 2),
        2 => ((b(1) & 0x7f) as i32, 2),
        3 => (((b(1) & 0x7f) as i32) | ((b(2) & 0x7f) as i32) << 7, 3),
        _ => (0, 0),
    }
}

fn u14(s: &[u8], q: usize) -> usize {
    (s.get(q).copied().unwrap_or(0) & 0x7f) as usize | ((s.get(q + 1).copied().unwrap_or(0) & 0x7f) as usize) << 7
}

fn sjis_pair(a: u8, b: u8) -> bool {
    matches!(a, 0x81..=0x9f | 0xe0..=0xfc) && matches!(b, 0x40..=0x7e | 0x80..=0xfc)
}

/// Expands a string: Shift-JIS bytes in and out. `item(id, count, no_article)` names an item for code 0x8e.
pub fn format(s: &[u8], strs: &[&[u8]], ints: &[i32], item: &dyn Fn(i32, i32, bool) -> Option<Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut eq, mut lt, mut gt) = (false, false, false);
    let mut end = 0usize;
    let mut i = 0usize;
    while i < s.len() && s[i] != 0 {
        let next = s.get(i + 1).copied().unwrap_or(0);
        if !sjis_pair(s[i], next) {
            out.push(s[i]);
            i += 1;
            continue;
        }
        if s[i] != 0xfa {
            out.extend_from_slice(&s[i..i + 2]);
            i += 2;
            continue;
        }
        let code = s.get(i + 2).copied().unwrap_or(0);
        let p = i + 3;
        i = match code {
            // A choice: the index picks an offset (from p) into the table; out of range jumps to its end.
            0x82 => {
                let (idx, n) = arg(s, p, ints);
                let q = p + n;
                let cnt = u14(s, q);
                end = p + u14(s, q + 2);
                if (0..cnt as i32).contains(&idx) { p + u14(s, q + 4 + 2 * idx as usize) } else { end }
            }
            0x83 => {
                let (a, n) = arg(s, p, ints);
                let (b, m) = arg(s, p + n, ints);
                (eq, lt, gt) = (a == b, a < b, a > b);
                p + n + m
            }
            0x84..=0x8a => {
                let l = s.get(p).copied().unwrap_or(0) as i8 as isize;
                let taken = match code {
                    0x84 => true,
                    0x85 => eq,
                    0x86 => !eq,
                    0x87 => gt,
                    0x88 => lt,
                    0x89 => gt || eq,
                    _ => lt || eq,
                };
                if taken { (p as isize + 1 + l).max(0) as usize } else { p + 1 }
            }
            0x8b => {
                let (v, n) = arg(s, p, ints);
                out.extend_from_slice(v.to_string().as_bytes());
                p + n
            }
            0x8c => {
                let (k, n) = arg(s, p, ints);
                if let Some(t) = strs.get(k as usize) {
                    out.extend_from_slice(t);
                }
                p + n
            }
            0x8d => p.max(end),
            0x8e => {
                let (obj, n1) = arg(s, p, ints);
                let (count, n2) = arg(s, p + n1, ints);
                let (noart, n3) = arg(s, p + n1 + n2, ints);
                if let Some(t) = item(obj, count, noart != 0) {
                    out.extend_from_slice(&t);
                }
                p + n1 + n2 + n3
            }
            _ => p,
        };
    }
    out
}

/// The format rows by key (tables 0x1037d31c date order, 0x1037d34c 12 / 24 hour): group 10 ids at +2 .. +10.
const DATE_ROWS: [[u16; 5]; 3] = [[0x3b, 0x4d, 0x48, 0x43, 0x43], [0x3c, 0x4e, 0x49, 0x44, 0x44], [0x3d, 0x4f, 0x4a, 0x45, 0x45]];
const TIME_ROWS: [[u16; 5]; 2] = [[0x3e, 0x3e, 0x4b, 0x46, 0x40], [0x3f, 0x3f, 0x4b, 0x46, 0x40]];

/// The Vana'diel calendar of a game time t (Earth seconds since the epoch, FUN_101039e0): year, month, day,
/// hour, minute, weekday (group 0x10), moon phase (group 0x11) and the moon percent (FUN_10103ba0).
pub struct VanaParts {
    pub year: i32,
    pub month: i32,
    pub day: i32,
    pub hour: i32,
    pub minute: i32,
    pub weekday: usize,
    pub phase: usize,
    pub moon: i32,
}

impl VanaParts {
    pub fn of(t: u64) -> Self {
        let d = ((t / 3456) % 84) as i32;
        let v = (d - 4) as f32 * 2.380_952_4_f32;
        let r = if v >= 0.0 { v + 0.5 } else { v - 0.5 };
        let moon = if d < 4 { (-r).trunc() as i32 } else if d <= 46 { r.trunc() as i32 } else { 200 - r.trunc() as i32 };
        VanaParts {
            year: (t / 0x12fc00) as i32 + 886,
            month: (t % 0x12fc00 / 0x19500) as i32 + 1,
            day: (t % 0x19500 / 0xd80) as i32 + 1,
            hour: ((t % 3456) / 144) as i32,
            minute: ((t % 144) * 60 / 144) as i32,
            weekday: ((t % 27648) / 3456) as usize,
            phase: ((t / 24192) % 12) as usize,
            moon,
        }
    }
}

/// Group 10 strings by id (raw Shift-JIS) for the builders.
pub type Group<'a> = dyn Fn(u16) -> Option<Vec<u8>> + 'a;

fn fmt(g: &Group<'_>, id: u16, strs: &[&[u8]], ints: &[i32]) -> Vec<u8> {
    g(id).map(|s| format(&s, strs, ints, &|_, _, _| None)).unwrap_or_default()
}

/// FUN_101c92b0(t, flags, raw = 1): the Vana'diel date (row +8 of the date key), then the time (0x46) unless flags & 2.
pub fn vana_datetime(g: &Group<'_>, t: u64, flags: u32, date_key: usize) -> Vec<u8> {
    let p = VanaParts::of(t);
    let Some(row) = DATE_ROWS.get(date_key) else { return b"--/--/--".to_vec() };
    let date = fmt(g, row[3], &[], &[p.year, p.month, p.day]);
    if flags & 2 != 0 {
        return date;
    }
    let time = fmt(g, TIME_ROWS[0][3], &[], &[p.hour, p.minute]);
    [date, b" ".to_vec(), time].concat()
}

/// FUN_101c8fd0(unix, flags): the Earth date in local time (flags & 1 the long form with the month's name, & 4 its
/// other name list), the time (flags & 8 without seconds), joined unless flags & 2 (date only) / & 0x18 (time only).
pub fn earth_datetime(g: &Group<'_>, unix: i64, flags: u32, date_key: usize, h12: bool) -> Vec<u8> {
    let (Some(row), Ok(ts)) = (DATE_ROWS.get(date_key), jiff::Timestamp::from_second(unix)) else {
        return if flags & 1 != 0 { b"----/--/-- --:--:--".to_vec() } else { b"--/--/-- --:--:--".to_vec() };
    };
    let z = ts.to_zoned(jiff::tz::TimeZone::system());
    let (year, month, day) = (z.year() as i32, z.month() as i32, z.day() as i32);
    let (hour, minute, second) = (z.hour() as i32, z.minute() as i32, z.second() as i32);
    let hour12 = if hour % 12 == 0 { 12 } else { hour % 12 };
    let date = if flags & 1 != 0 {
        let name = fmt(g, if flags & 4 != 0 { 0x61 } else { 0x60 }, &[], &[month]);
        fmt(g, row[0], &[&name], &[year, day])
    } else {
        let m = month.to_string();
        fmt(g, row[1], &[m.as_bytes()], &[year % 100, day])
    };
    let trow = &TIME_ROWS[h12 as usize];
    let time = if flags & 8 != 0 { fmt(g, trow[4], &[], &[hour, hour12, minute]) } else { fmt(g, trow[0], &[], &[hour, hour12, minute, second]) };
    if flags & 2 != 0 {
        date
    } else if flags & 0x18 != 0 {
        time
    } else {
        [date, b" ".to_vec(), time].concat()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group10() -> Option<super::super::msg::Messages> {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).ok()?;
        let dll = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")).ok()?;
        super::super::msg::Messages::load(&index, super::super::msg::group_file(&dll, 1, 10)?).ok()
    }

    fn s(b: Vec<u8>) -> String {
        crate::dat::font::decode_sjis(&b)
    }

    /// The group 10 rows of the spec's table, on the real file.
    #[test]
    fn date_time_rows() {
        let Some(m) = group10() else { return };
        let f = |id: usize, strs: &[&[u8]], ints: &[i32]| s(format(&m.raw(id).unwrap(), strs, ints, &|_, _, _| None));
        assert_eq!(f(0x3b, &[b"Oct."], &[2026, 23]), "2026, Oct. 23");
        assert_eq!(f(0x3c, &[b"Oct."], &[2026, 23]), "Oct. 23, 2026");
        assert_eq!(f(0x3e, &[], &[19, 7, 45, 12]), "19:45:12");
        assert_eq!(f(0x3f, &[], &[19, 7, 45, 12]), "7:45:12pm");
        assert_eq!(f(0x3f, &[], &[0, 12, 5, 9]), "12:05:09am");
        assert_eq!(f(0x43, &[], &[1121, 4, 5]), "1121/4/5");
        assert_eq!(f(0x46, &[], &[7, 5]), "7:05");
        assert_eq!(f(0x60, &[], &[10]), "Oct.");
        assert_eq!(f(0x60, &[], &[5]), "May");
    }

    /// The spec's example: Earth 2026-10-23 19:45:12 CDT = unix 1792802712, Vana t = 782991912.
    #[test]
    fn clock_example() {
        let Some(m) = group10() else { return };
        let g = |id: u16| m.raw(id as usize);
        assert_eq!(s(vana_datetime(&g, 782_991_912, 2, 0)), "1515/5/1");
        let p = VanaParts::of(782_991_912);
        assert_eq!((p.weekday, p.hour, p.minute, p.phase, p.moon), (0, 3, 50, 1, 19));
        // Local time differs per machine: only the shape is checked.
        let e = s(earth_datetime(&g, 1_792_802_712, 1, 0, false));
        assert!(e.starts_with("2026, Oct. 2") && e.matches(':').count() == 2, "{e}");
    }

    /// The moon percent at the turning points (FUN_10103ba0).
    #[test]
    fn moon_percent() {
        let at = |d: u64| VanaParts::of(d * 3456).moon;
        assert_eq!([at(0), at(3), at(4), at(46), at(47), at(83)], [10, 2, 0, 100, 98, 12]);
    }
}
