//! Zone dialogue (event message) files, as FUN_100ac7b0 / FUN_100890c0 load them and FUN_10094aa0 reads
//! them: u32 `size | 0x10000000`, then (XOR 0x80 when byte 3 is 0x10) a table of u32 offsets relative to
//! +4, count = first offset / 4. Strings carry the control codes FUN_100bf790 expands.

use super::DatIndex;
use super::font::decode_sjis;
use anyhow::{Context, Result, bail};

/// English file id of a zone's dialogue (FUN_100ac7b0, language slots 0x68 / 0x6b / 0x69 / 0x6a).
pub fn file_id(zone: u16) -> u32 {
    let z = zone as u32;
    match z {
        0..256 => 6420 + z,
        256..1000 => 85591 + z - 256,
        1000..2000 => 57945 + z - 1000,
        _ => 68511 + z - 2000,
    }
}

pub struct Dialog {
    entries: Vec<Vec<u8>>,
}

impl Dialog {
    pub fn load(index: &DatIndex, zone: u16) -> Result<Dialog> {
        let id = file_id(zone);
        Self::parse(&index.read(id)?).with_context(|| format!("dialogue file {id} (zone {zone})"))
    }

    pub fn parse(file: &[u8]) -> Result<Dialog> {
        if file.len() < 8 {
            bail!("too short");
        }
        let mut d = file.to_vec();
        if d[3] == 0x10 {
            d[4..].iter_mut().for_each(|b| *b ^= 0x80);
        }
        let body = &d[4..];
        let u32_at = |o: usize| body.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize);
        let count = u32_at(0).context("no offset table")? / 4;
        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let start = u32_at(i * 4).context("offset table")?;
            let end = if i + 1 < count { u32_at(i * 4 + 4).context("offset table")? } else { body.len() };
            entries.push(body.get(start..end.max(start)).context("string out of range")?.to_vec());
        }
        Ok(Dialog { entries })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn raw(&self, id: u32) -> Option<Vec<u8>> {
        self.entries.get(id as usize).cloned()
    }

    /// The text with control codes shown as `{tags}`; `[a/b]` alternatives are kept as written.
    pub fn get(&self, id: u32) -> Option<String> {
        Some(readable(self.entries.get(id as usize)?))
    }
}

/// Length of a `[a/b]` group starting the search at `from` (FUN_100bbc10): up to and past ']'.
fn bracket_end(s: &[u8], from: usize) -> usize {
    let Some(open) = s[from.min(s.len())..].iter().position(|&b| b == b'[') else { return s.len() };
    let mut i = from + open + 1;
    while i < s.len() && s[i] != b']' && s[i] != 0 {
        i += if s[i] >= 0x80 { 2 } else { 1 };
    }
    (i + 1).min(s.len())
}

/// A grammar argument (FUN_100bbd20): 0x40|k = slot k; 0x?2 = 14-bit index packed 7 bits per byte.
fn grammar_arg(s: &[u8]) -> String {
    match s {
        [b, ..] if b & 0x40 != 0 => format!("slot{}", b & 0x3f),
        [b, b1, b2, b3, ..] if b & 0x3f == 2 => (((((b2 & 0x7f) as u16) | (*b3 as u16) << 7) << 7) | (b1 & 0x7f) as u16).to_string(),
        _ => "?".into(),
    }
}

/// A 0x01 block (FUN_100bc8a0, English): `01 len kind args`.
fn grammar(kind: u8, args: &[u8]) -> String {
    let arg = grammar_arg(args);
    let src = |k: u8| ["#", "num", "work"][(k / 3) as usize % 3];
    let form = |k: u8| ["", " a", " pl"][(k % 3) as usize];
    match kind {
        // Item records (0xc00 bytes, FUN_101550f0): name / with article / plural; index, num[] or work[].
        0x20..=0x28 => format!("{{item{} {}{arg}}}", form(kind - 0x20), src(kind - 0x20)),
        // Names through FUN_1014e180 (key items, inferred).
        0x30..=0x35 => format!("{{keyitem{} {}{arg}}}", form(kind - 0x30), src(kind - 0x30)),
        0x42..=0x44 => format!("{{keyitem{} work{arg}}}", form(kind - 0x42)),
        // With its own article (current FUN_100be760).
        0x36 => format!("{{keyitem art num{arg}}}"),
        0x45 => format!("{{keyitem art work{arg}}}"),
        0x03 => format!("{{count num{arg}}}"),
        0x04 => format!("{{count work{arg}}}"),
        _ => format!("{{grammar {kind:02x} {arg}}}"),
    }
}

/// Renders a dialogue string readably, stepping over codes with FUN_100bf790's lengths.
pub fn readable(s: &[u8]) -> String {
    let mut out = String::new();
    let mut text: Vec<u8> = Vec::new();
    let mut i = 0;
    let arg = |i: usize| s.get(i + 1).copied().unwrap_or(0);
    while i < s.len() {
        let c = s[i];
        if c >= 0x80 {
            text.extend_from_slice(&s[i..(i + 2).min(s.len())]);
            i += 2;
            continue;
        }
        if c >= 0x20 && c != 0x7f {
            text.push(c);
            i += 1;
            continue;
        }
        out.push_str(&decode_sjis(&text));
        text.clear();
        let (tag, len) = match c {
            0x00 => break,
            0x01 => {
                let n = arg(i) as usize;
                let body = s.get(i + 2..(i + 2 + n).min(s.len())).unwrap_or_default();
                (body.first().map_or("{grammar}".into(), |&k| grammar(k, &body[1..])), n + 2)
            }
            0x02 | 0x03 => (format!("{{{c:02x} {}}}", u16::from_le_bytes([arg(i), s.get(i + 2).copied().unwrap_or(0)])), 3),
            0x05 => (format!("{{text2 work{}}}", arg(i)), 2),
            0x07 => ("\n".into(), 1),
            0x08 => ("{player}".into(), 1),
            0x09 => ("{speaker}".into(), 1),
            0x0a => (format!("{{num{}}}", arg(i)), 2),
            0x0b => ("{choice}".into(), 1),
            0x0c => (format!("{{select num{}}}", arg(i)), 2),
            0x0d => (format!("{{sound work{}}}", arg(i)), 2),
            0x0e => (format!("{{sound num{}}}", arg(i)), 2),
            0x10 => (format!("{{name work{}}}", arg(i)), 2),
            0x11 => (format!("{{name num{}}}", arg(i)), 2),
            0x12 => (format!("{{number work{}}}", arg(i)), 2),
            0x14 => (format!("{{time work{}}}", arg(i)), 2),
            0x16 => (format!("{{name16 work{}}}", arg(i)), 2),
            0x1a => (format!("{{name16 num{}}}", arg(i)), 2),
            0x17 => (format!("{{emote {}}}", arg(i)), 2),
            0x1c => (format!("{{string{}}}", arg(i)), 2),
            0x1d => ("{direction}".into(), 1),
            0x1e => (format!("{{color {:02x}}}", arg(i)), 2),
            0x1f => (format!("{{mode {:02x}}}", arg(i)), 2),
            0x13 | 0x18 | 0x19 => (format!("{{{c:02x} {}}}", arg(i)), 2),
            0x7f => sub_code(s, i),
            _ => (format!("{{{c:02x}}}"), 1),
        };
        out.push_str(&tag);
        i += len.max(1);
    }
    out.push_str(&decode_sjis(&text));
    out
}

/// `7f xx ...` codes: expander lengths (FUN_100bf790), display meaning from FUN_10123060.
fn sub_code(s: &[u8], i: usize) -> (String, usize) {
    let b = |k: usize| s.get(i + k).copied().unwrap_or(0);
    let sub = b(1);
    // Codes whose argument picks one of the `[a/b]` alternatives that follow.
    let pick = |what: String| -> (String, usize) {
        let end = bracket_end(s, i);
        let open = s[i..end].iter().position(|&c| c == b'[').map_or(end, |p| i + p);
        (format!("{{{what}}}{}", decode_sjis(&s[open..end])), end - i)
    };
    match sub {
        0x31 => ("{next}".into(), 2),
        0x32 | 0x33 | 0x37 => (format!("{{7f{sub:02x}}}"), 2),
        0x34 => (format!("{{wait {}}}", b(2)), 3),
        0x35 | 0x36 => (format!("{{7f{sub:02x} {}}}", b(2)), 3),
        0x38 => (format!("{{wait {}}}", u16::from_le_bytes([b(2), b(3)])), 4),
        0x80 => (format!("{{capital {}}}", b(2)), 3),
        0x81 => (format!("{{7f81 num{}}}", b(2)), 3),
        0x84 => (format!("{{list {}}}", b(2)), 3),
        0x85 => pick("player m/f".into()),
        0x86 => pick(format!("plural work{}", b(2))),
        0x92 => pick(format!("plural num{}", b(2))),
        0x87 | 0x88 => pick(format!("7f{sub:02x} {}", b(2))),
        0x90 => pick("speaker m/f".into()),
        0x91 => pick("target m/f".into()),
        0x8f => (format!("{{name8f work{}}}", b(2)), 3),
        0x93 => ("{msgstring}".into(), 2),
        0x94 => (format!("{{num02 num{}}}", b(2)), 3),
        0x95 => (format!("{{hex num{}}}", b(2)), 3),
        0x96 => (format!("{{binary num{}}}", b(2)), 3),
        0x97 => (format!("{{num work{}}}", b(2)), 3),
        0xa0..=0xab => (format!("{{date{:x} num{}}}", sub - 0xa0, b(2)), 3),
        0xac | 0xb0 | 0xb1 | 0xb4 | 0xb5 => (format!("{{7f{sub:02x} {}}}", b(2)), 3),
        _ => (format!("{{7f{sub:02x}}}"), 2),
    }
}

/// What the client fills codes from: the message object's params (+0xc64 num, +0xd04 strings).
#[derive(Default)]
pub struct Ctx {
    pub player: String,
    pub speaker: String,
    pub num: [i32; 8],
    /// num[8..40] (+0xc84): the VM's G registers (0x1700.., DAT_10488980), copied into every message (FUN_100c2e20).
    pub g: [i32; 32],
    pub strings: Vec<String>,
    pub female: bool,
    /// Message work[5] (msg+0xc50, DAT_10474618), read by the "work" grammar forms.
    pub work: [i32; 5],
    /// Item, key item and other names for 0x01 grammar blocks.
    pub names: Option<std::sync::Arc<dyn super::names::NameSource>>,
    /// The date builders' group 10 strings and settings (`7f ab` / `7f ac`).
    pub dates: Option<std::sync::Arc<Dates>>,
    /// msg+0xc10: the name the line was queued with (7f 93; the speaker for 0x036 / 0x02A, none for 0x027).
    pub msg_name: String,
    /// The party roster (G+0x57d8) for 0x18 / 0x19: (record, id, MemberNumber, name) in display order; our id.
    pub party: Vec<(u8, u32, u8, String)>,
    pub me: u32,
    /// The client's other name tables.
    pub texts: Option<std::sync::Arc<dyn Texts>>,
}

/// Name tables FUN_100c1650 reads besides `names`.
pub trait Texts: Send + Sync {
    /// FUN_100964a0(kind, id): 2 combat skills (05), 3 list names (7f 84), 7 monster abilities (16 / 1a from 0x100).
    fn kind(&self, kind: u8, id: u32) -> Option<Vec<u8>>;
    /// The name of work[n]: 7f b0 a merit or merit category (FUN_10161070), b1 a job point entry, b7 an alter ego
    /// point entry.
    fn point(&self, sub: u8, id: u16) -> Option<Vec<u8>>;
    /// 7f b5: row num[n] of language slot 0x21 (Monstrosity), FUN_100bd9b0.
    fn monstrosity(&self, row: i16) -> Option<Vec<u8>>;
}

/// Group 10 by id (raw) and config 0x95 (the date order) / 0x96 (12-hour).
pub struct Dates {
    pub group10: Box<dyn Fn(u16) -> Option<Vec<u8>> + Send + Sync>,
    pub date_key: usize,
    pub h12: bool,
}

impl Ctx {
    /// num[a] of the message's 40.
    fn num_at(&self, a: usize) -> Option<i32> {
        self.num.get(a).or_else(|| self.g.get(a.wrapping_sub(8))).copied()
    }
}

/// Article pending for the next noun (FUN_1024fcd0): line, byte position, definite.
type Pending = Option<(usize, usize, bool)>;

/// A 0x01 grammar block (FUN_100bc8a0, English phrase builder FUN_1024f820).
fn grammar_expand(kind: u8, args: &[u8], ctx: &Ctx, lines: &mut [Vec<u8>], pending: &mut Pending) {
    use super::font::encode_sjis;
    // FUN_100bbd20: 0x?2 = packed index; slot args (0x40) are not filled here.
    let arg = match args {
        [b, b1, b2, b3, ..] if b & 0x40 == 0 && b & 0x3f == 2 => Some(((((b2 & 0x7f) as u16) | (*b3 as u16) << 7) << 7) | (b1 & 0x7f) as u16),
        _ => None,
    };
    let at = |k: u8| -> Option<u32> {
        let a = arg? as usize;
        match k {
            0 => Some(a as u32),
            1 => ctx.num_at(a).map(|v| v as u32),
            _ => ctx.work.get(a).map(|&v| v as u32),
        }
    };
    let line = lines.len() - 1;
    let push = |lines: &mut [Vec<u8>], t: &str| lines[line].extend_from_slice(&encode_sjis(t));
    let names = ctx.names.as_deref();
    // FUN_100be760 wraps each name in `1e c` .. `1e 01` (FUN_100be680 / FUN_100be690), after its article: items 2, key
    // items 3, Records of Eminence objectives 8.
    let noun = |lines: &mut [Vec<u8>], pending: &mut Pending, n: super::names::Noun, form: u8, colour: u8| {
        let (text, mode, an) = match form {
            0 => (n.bare, n.mode, n.an),
            1 => (n.singular, 3, n.singular_an),
            _ => (n.plural, 4, false),
        };
        if let Some((l, pos, definite)) = pending.take() {
            let art = match (definite, mode) {
                (true, _) => "the",
                (false, 3) => if an { "an" } else { "a" },
                (false, _) => "some",
            };
            if l < lines.len() && pos <= lines[l].len() {
                let a = encode_sjis(art);
                lines[l].splice(pos..pos, a);
            }
        }
        lines[line].extend_from_slice(&[0x1e, colour]);
        push(lines, &text);
        lines[line].extend_from_slice(&[0x1e, 1]);
    };
    match kind {
        0x01 | 0x02 => *pending = Some((line, lines[line].len(), kind == 0x02)),
        0x03 | 0x04 => match at(if kind == 0x03 { 1 } else { 2 }) {
            Some(1) | None => *pending = Some((line, lines[line].len(), false)),
            Some(v) => push(lines, &v.to_string()),
        },
        0x20..=0x28 => {
            let k = kind - 0x20;
            if let Some(n) = at(k / 3).and_then(|id| names?.item(id)) {
                noun(lines, pending, n, k % 3, 2);
            }
        }
        // 0x29 / 0x2a (FUN_100be760): a count index then an item index, both from num (0x29) or work (0x2a); one is
        // the singular, else the plural.
        0x29 | 0x2a => {
            let idx = |b: &[u8]| match b {
                [b0, b1, b2, b3, ..] if b0 & 0x40 == 0 && b0 & 0x3f == 2 => Some((((((b2 & 0x7f) as u16) | (*b3 as u16) << 7) << 7) | (b1 & 0x7f) as u16) as usize),
                _ => None,
            };
            let val = |i: Option<usize>| -> Option<u32> {
                let i = i?;
                if kind == 0x29 { ctx.num_at(i).map(|v| v as u32) } else { ctx.work.get(i).map(|&v| v as u32) }
            };
            let count = val(idx(args));
            let item = val(args.get(4..).and_then(idx));
            if let Some(n) = item.and_then(|id| names?.item(id)) {
                noun(lines, pending, n, if count == Some(1) { 1 } else { 2 }, 2);
            }
        }
        0x30..=0x35 | 0x42..=0x44 => {
            let k = if kind >= 0x42 { kind - 0x42 + 6 } else { kind - 0x30 };
            if let Some(n) = at(k / 3).and_then(|id| names?.key_item(id)) {
                noun(lines, pending, n, k % 3, 3);
            }
        }
        // 0x36 / 0x45 (current FUN_100be760): a key item from num[] / work[] with its own article, "the" for key item
        // types 0 / 1 (FUN_101552e0), else "a" / "an" / none by the entry's mode (FUN_101540a0).
        0x36 | 0x45 => {
            if let Some(id) = at(if kind == 0x36 { 1 } else { 2 }).filter(|&id| id != 0)
                && let Some(n) = names.and_then(|s| s.key_item(id))
            {
                let art = match (names.is_some_and(|s| s.key_item_definite(id)), n.mode) {
                    (true, _) => "the",
                    (false, 3) if n.an => "an",
                    (false, 3) => "a",
                    _ => "some",
                };
                push(lines, &format!("{art} "));
                lines[line].extend_from_slice(&[0x1e, 3]);
                push(lines, &n.bare);
                lines[line].extend_from_slice(&[0x1e, 1]);
            }
        }
        // Records of Eminence objectives: item id + 0xe000 (FUN_100bc8a0 0x39 / 0x40 / 0x41).
        0x39 | 0x40 | 0x41 => {
            let src = match kind {
                0x39 => 0,
                0x40 => 1,
                _ => 2,
            };
            if let Some(n) = at(src).and_then(|id| names?.item(id + 0xe000)) {
                noun(lines, pending, n, 0, 8);
            }
        }
        // Message group 0x14: place names.
        0x37 | 0x38 => {
            if let Some(t) = at(if kind == 0x37 { 0 } else { 1 }).and_then(|id| names?.place(id)) {
                push(lines, &t);
            }
        }
        0x12 => {
            if let Some(t) = at(1).and_then(|id| names?.title(id)) {
                push(lines, &t);
            }
        }
        _ => {}
    }
}

/// An expanded message: Shift-JIS lines and how it ends.
pub struct Expanded {
    pub lines: Vec<Vec<u8>>,
    /// Ends in `7f 31`: the log waits for a key with the kaipage arrow (FUN_10123060 type 0x8001).
    pub prompt: bool,
    /// Text before 0x0b and the options after it (FUN_100b0960 splits at 07).
    pub choice: Option<usize>,
    /// Timed close: `7f 34 n` / `7f 35 n` wait n x 60 ticks, `7f 38 lo hi` a u16 of ticks (type 0x8002).
    pub wait: Option<u32>,
    /// `02 lo hi` / `03 lo hi`: event message window x / y (DAT_10476f98 / 9a, FUN_100bf790 -> FUN_10132030).
    pub place: [Option<i16>; 2],
    /// A `1f xx` code: the log mode the line takes (overrides the packet's).
    pub mode: Option<u8>,
    /// `0d n` / `0e n`: SeSep se ids of work[n] / num[n], played as the line is built (FUN_100382c0, pan 0x40, 0x7f).
    pub sounds: Vec<u32>,
}

/// Expands the codes a plain NPC line uses; others are dropped. Not the full FUN_100bf790.
pub fn expand(s: &[u8], ctx: &Ctx) -> Expanded {
    use super::font::encode_sjis;
    let mut lines = vec![Vec::new()];
    let (mut prompt, mut choice, mut wait) = (false, None, None);
    let mut sounds = Vec::new();
    let work = |n: usize| ctx.work.get(n).copied().unwrap_or(0);
    let num = |n: usize| ctx.num_at(n).unwrap_or(0);
    let texts = ctx.texts.as_deref();
    let mut place = [None; 2];
    let mut mode = None;
    let mut pending: Pending = None;
    // `7f 80 n` (FUN_100bf790 case 0x80 -> expander +0xc): the next name's first letter in upper case for n = 1
    // (FUN_100c0c40); the next grammar block or name clears it.
    let mut cap: Option<(usize, usize)> = None;
    let capital = |lines: &mut Vec<Vec<u8>>, cap: &mut Option<(usize, usize)>| {
        if let Some((l, mut pos)) = cap.take()
            && let Some(line) = lines.get_mut(l)
        {
            // FUN_100c2bf0 passes colour codes by.
            while line.get(pos) == Some(&0x1e) {
                pos += 2;
            }
            if let Some(b) = line.get_mut(pos) {
                b.make_ascii_uppercase();
            }
        }
    };
    // The last `1e xx`: a new line starts with it again (the wrapper FUN_1012a4f0 carries it, 1 = the line colour).
    let mut colour = 1u8;
    let mut i = 0;
    let push = |lines: &mut Vec<Vec<u8>>, b: &[u8]| lines.last_mut().unwrap().extend_from_slice(b);
    let alt = |from: usize, k: usize| -> (Vec<u8>, usize) {
        let end = bracket_end(s, from);
        let open = s[from..end].iter().position(|&c| c == b'[').map_or(end, |p| from + p + 1);
        let inner = &s[open..end.saturating_sub(1).max(open)];
        (inner.split(|&c| c == b'/').nth(k).unwrap_or_default().to_vec(), end - from)
    };
    while i < s.len() {
        let c = s[i];
        let a = s.get(i + 1).copied().unwrap_or(0) as usize;
        let step = match c {
            0x80.. => {
                push(&mut lines, &s[i..(i + 2).min(s.len())]);
                2
            }
            0x00 => break,
            0x07 => {
                lines.push(if colour == 1 { Vec::new() } else { vec![0x1e, colour] });
                1
            }
            0x08 => {
                push(&mut lines, &encode_sjis(&ctx.player));
                1
            }
            0x09 => {
                push(&mut lines, &encode_sjis(&ctx.speaker));
                1
            }
            0x0a => {
                push(&mut lines, ctx.num_at(a).unwrap_or(0).to_string().as_bytes());
                2
            }
            0x0b => {
                choice.get_or_insert(lines.len() - 1);
                1
            }
            0x0c => {
                let (t, n) = alt(i + 2, ctx.num_at(a).unwrap_or(0).max(0) as usize);
                push(&mut lines, &t);
                n + 2
            }
            0x1c => {
                push(&mut lines, &encode_sjis(ctx.strings.get(a).map_or("", |s| s)));
                2
            }
            0x01 => {
                let body = s.get(i + 2..(i + 2 + a).min(s.len())).unwrap_or_default();
                if let Some(&k) = body.first() {
                    grammar_expand(k, &body[1..], ctx, &mut lines, &mut pending);
                }
                capital(&mut lines, &mut cap);
                a + 2
            }
            0x02 | 0x03 => {
                place[(c - 2) as usize] = Some(i16::from_le_bytes([a as u8, s.get(i + 2).copied().unwrap_or(0)]));
                3
            }
            0x10 | 0x11 | 0x16 | 0x1a => {
                let v = if c == 0x10 || c == 0x16 { ctx.work.get(a).copied() } else { ctx.num_at(a) };
                let names = ctx.names.as_deref();
                let t = match (c, v.map(|v| v as u16 as u32)) {
                    (0x10 | 0x11, Some(id)) => names.and_then(|n| n.spell(id)).map(|t| encode_sjis(&t)),
                    (_, Some(id)) if id < 0x100 => names.and_then(|n| n.ability(id)).map(|t| encode_sjis(&t)),
                    // Kind 7 (monster abilities) from 0x100.
                    (_, Some(id)) => texts.and_then(|t| t.kind(7, id - 0x100)),
                    _ => None,
                };
                if let Some(t) = t {
                    push(&mut lines, &t);
                }
                capital(&mut lines, &mut cap);
                2
            }
            0x1f => {
                mode.get_or_insert(a as u8);
                2
            }
            // 05 n: kind-2 text (combat skills) of work[n] (FUN_100964a0(2, ..)).
            0x05 => {
                if let Some(t) = texts.and_then(|t| t.kind(2, work(a) as u16 as u32)) {
                    push(&mut lines, &t);
                }
                2
            }
            0x0d | 0x0e => {
                sounds.push(if c == 0x0d { work(a) } else { num(a) } as u32);
                2
            }
            // 12 n: work[n] "%d" (separators only for languages 2 / 3, FUN_101c9440).
            0x12 => {
                push(&mut lines, work(a).to_string().as_bytes());
                2
            }
            0x14 => {
                let t = work(a);
                push(&mut lines, format!("{}:{:02}:{:02}", t / 3600, (t % 3600 / 60) as u32, (t % 60) as u32).as_bytes());
                2
            }
            // 18 n (FUN_100e8df0): the roster record whose MemberNumber is n (n = 0: ours); 19 n (FUN_100e8db0): record n.
            0x18 | 0x19 => {
                let m = ctx.party.iter().find(|m| match (c, a) {
                    (0x18, 0) => m.1 == ctx.me,
                    (0x18, _) => m.2 as usize == a,
                    _ => m.0 as usize == a,
                });
                if let Some(m) = m.filter(|m| !m.3.is_empty()) {
                    push(&mut lines, &encode_sjis(&m.3));
                }
                2
            }
            0x1e => {
                push(&mut lines, &[0x1e, a as u8]);
                colour = a as u8;
                2
            }
            0x10..=0x19 | 0x1b => 2,
            0x7f => match a as u8 {
                0x31 => {
                    prompt = true;
                    2
                }
                0x85 | 0x90 | 0x91 => {
                    let (t, n) = alt(i, ctx.female as usize);
                    push(&mut lines, &t);
                    n
                }
                // Plurals by work[n] (0x86, msg +0xc50) / num[n] (0x92, +0xc64): English takes the second form
                // unless the value is 1 (Japanese and language 3: above 1).
                0x86 | 0x92 => {
                    let n = s.get(i + 2).copied().unwrap_or(0) as usize;
                    let v = if a as u8 == 0x86 { ctx.work.get(n).copied() } else { ctx.num_at(n) };
                    let (t, len) = alt(i, (v.unwrap_or(0) != 1) as usize);
                    push(&mut lines, &t);
                    len
                }
                0x87 | 0x88 => alt(i, 0).1,
                0x34 | 0x35 => {
                    wait = Some(s.get(i + 2).copied().unwrap_or(0) as u32 * 60);
                    3
                }
                0x38 => {
                    wait = Some(u16::from_le_bytes([s.get(i + 2).copied().unwrap_or(0), s.get(i + 3).copied().unwrap_or(0)]) as u32);
                    4
                }
                0x80 => {
                    if s.get(i + 2) == Some(&1) {
                        cap = Some((lines.len() - 1, lines.last().map_or(0, Vec::len)));
                    }
                    3
                }
                // FUN_100c1650 (asm 0x100c1e36..0x100c202f): num[n] + 0x3c307d70 as Earth local time (FUN_101c8d10): a0 the
                // year, a1..a5 month / day / hour / minute / second "%d", a6..aa the same "%02d"; ab the long Earth date
                // (FUN_101c8fd0 flags 1); ac the Vana'diel date and time of num[n] (FUN_101c92b0(t, 1, 1)).
                0xa0..=0xac => {
                    let v = ctx.num_at(s.get(i + 2).copied().unwrap_or(0) as usize).unwrap_or(0);
                    let unix = v as i64 + 0x3c30_7d70;
                    let sub = a as u8;
                    let text = match (sub, &ctx.dates) {
                        (0xab, Some(d)) => super::msgfmt::earth_datetime(&*d.group10, unix, 1, d.date_key, d.h12),
                        (0xac, Some(d)) => super::msgfmt::vana_datetime(&*d.group10, v as u32 as u64, 1, d.date_key),
                        (0xab | 0xac, None) => Vec::new(),
                        _ => super::msgfmt::earth_parts(unix).map_or_else(Vec::new, |p| {
                            let k = if sub == 0xa0 { 0 } else { (sub - 0xa1) as usize % 5 + 1 };
                            if sub >= 0xa6 { format!("{:02}", p[k]) } else { p[k].to_string() }.into_bytes()
                        }),
                    };
                    push(&mut lines, &text);
                    3
                }
                // 7f 84 n: the kind-3 names of work[4]'s set bits (n x 0x20 + bit), space separated.
                0x84 => {
                    let base = s.get(i + 2).copied().unwrap_or(0) as u32 * 0x20;
                    let names: Vec<Vec<u8>> = (0..32).filter(|b| work(4) as u32 >> b & 1 != 0).filter_map(|b| texts.and_then(|t| t.kind(3, base + b))).collect();
                    push(&mut lines, &names.join(&b' '));
                    3
                }
                // 7f 8f n: the job ability of work[n] (FUN_100bd960: id + 0x200).
                0x8f => {
                    let id = (work(s.get(i + 2).copied().unwrap_or(0) as usize) as i16).clamp(0, 0x10ff) as u32;
                    if let Some(t) = ctx.names.as_deref().and_then(|n| n.ability(id + 0x200)) {
                        push(&mut lines, &encode_sjis(&t));
                    }
                    3
                }
                0x93 => {
                    push(&mut lines, &encode_sjis(&ctx.msg_name));
                    2
                }
                // 7f 94 / 98 / 99 / 9a n: num[n] "%02d" / "%03d" / "%04d" / "%05d"; 95 "$%x" of num[n], 97 of work[n]; 96
                // "%" and num[n]'s binary digits; 9b work[n] (unsigned) x 0.1 "%.1f".
                0x94..=0x9b => {
                    let n = s.get(i + 2).copied().unwrap_or(0) as usize;
                    let t = match a as u8 {
                        0x94 => format!("{:02}", num(n)),
                        0x98 => format!("{:03}", num(n)),
                        0x99 => format!("{:04}", num(n)),
                        0x9a => format!("{:05}", num(n)),
                        0x95 => format!("${:x}", num(n)),
                        0x96 => format!("%{:b}", num(n)),
                        0x97 => format!("${:x}", work(n)),
                        _ => format!("{:.1}", work(n) as u32 as f64 * 0.1),
                    };
                    push(&mut lines, t.as_bytes());
                    3
                }
                0xb0 | 0xb1 | 0xb7 => {
                    let id = work(s.get(i + 2).copied().unwrap_or(0) as usize) as u16;
                    if let Some(t) = texts.and_then(|t| t.point(a as u8, id)) {
                        push(&mut lines, &t);
                    }
                    3
                }
                // 7f b4 n: work[n] in 0..999999999 with separators (FUN_101c9440), group 10 0x5f "%s gil".
                0xb4 => {
                    let v = work(s.get(i + 2).copied().unwrap_or(0) as usize).clamp(0, 999_999_999) as u32;
                    push(&mut lines, format!("{} gil", super::battle::separated(v)).as_bytes());
                    3
                }
                0xb5 => {
                    if let Some(t) = texts.and_then(|t| t.monstrosity(num(s.get(i + 2).copied().unwrap_or(0) as usize) as i16)) {
                        push(&mut lines, &t);
                    }
                    3
                }
                0x36 | 0x81..=0x83 | 0xad..=0xb6 => 3,
                _ => 2,
            },
            0x20..0x7f => {
                push(&mut lines, &[c]);
                1
            }
            _ => 1,
        };
        i += step;
    }
    // A line of colour codes alone is blank.
    while lines.len() > 1 && lines.last().is_some_and(|l| l.chunks(2).all(|c| c.len() == 2 && c[0] == 0x1e)) {
        lines.pop();
    }
    Expanded { lines, prompt, choice, wait, place, mode, sounds }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zone 5 7410: `7f ac 0` the Vana'diel date and time of num[0], `7f ab 0` the long Earth date (group 10 rows).
    #[test]
    fn date_builder_codes() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(dll) = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")) else { return };
        let Some(file) = super::super::msg::group_file(&dll, 1, 10) else { return };
        let m = std::sync::Arc::new(super::super::msg::Messages::load(&index, file).unwrap());
        let g = m.clone();
        let dates = std::sync::Arc::new(Dates { group10: Box::new(move |id: u16| g.raw(id as usize)), date_key: 0, h12: false });
        let t = 800_000_000;
        let d = Dialog::load(&index, 5).unwrap();
        let e = expand(&d.raw(7410).unwrap(), &Ctx { num: [t, 0, 0, 0, 0, 0, 0, 0], dates: Some(dates.clone()), ..Default::default() });
        let vana = decode_sjis(&crate::dat::msgfmt::vana_datetime(&*dates.group10, t as u64, 1, 0));
        let earth = decode_sjis(&crate::dat::msgfmt::earth_datetime(&*dates.group10, t as i64 + 0x3c30_7d70, 1, 0, false));
        let line = e.lines.iter().map(|l| decode_sjis(l)).collect::<Vec<_>>().join(" ");
        assert!(line.contains(&format!("until {vana} ({earth} Earth time)")), "{line}");
    }

    /// Zone 1 6380, the Assist Channel's end: `7f a1` / `a2` / `a0` / `a3` "%d" and `7f a9` "%02d" of num[0] +
    /// 0x3c307d70 in local time.
    #[test]
    fn earth_date_codes() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let d = Dialog::load(&index, 1).unwrap();
        let t = 800_000_000;
        let e = expand(&d.raw(6380).unwrap(), &Ctx { num: [t, 0, 0, 0, 0, 0, 0, 0], ..Default::default() });
        let p = crate::dat::msgfmt::earth_parts(t as i64 + 0x3c30_7d70).unwrap();
        let want = format!("You will be able to use the Assist Channel until {}/{}/{} at {}:{:02} (JST).", p[1], p[2], p[0], p[3], p[4]);
        assert_eq!(decode_sjis(&e.lines[0]), want);
    }

    /// s2c 0x027 lines: Port Bastok 7164 (fishing, String1 = the player, Num1 = item, count) and Valkurm Dunes 8099
    /// (a 1f mode code).
    #[test]
    fn talknumwork2_lines() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let names = crate::dat::names::Names::load(&index, crate::dat::items::Lang::En).ok().map(|n| std::sync::Arc::new(n) as std::sync::Arc<dyn crate::dat::names::NameSource>);
        let ctx = |num0| Ctx { strings: vec!["Lnxtest".into()], num: [num0, 1, 0, 0, 0, 0, 0, 0], names: names.clone(), ..Default::default() };
        let d = Dialog::load(&index, 236).unwrap();
        let e = expand(&d.raw(7164).unwrap(), &ctx(4401));
        assert_eq!(crate::dat::font::strip_codes(&crate::dat::font::decode_sjis(&e.lines[0])), "Lnxtest caught a moat carp!");
        let d = Dialog::load(&index, 103).unwrap();
        let e = expand(&d.raw(8099).unwrap(), &ctx(4112));
        assert_eq!((crate::dat::font::strip_codes(&crate::dat::font::decode_sjis(&e.lines[0])).as_str(), e.mode), ("Lnxtest obtains a potion!", Some(0x79)));
    }

    /// Selbina 6865 (Isacio, Elder Memories): `7f 80 01` then kind 0x36 on num[0] = key item 2825 (the script's
    /// `mov Z2, #2825`; num[] are Z2..Z9): type 2, so "a", capitalized.
    #[test]
    fn key_item_by_num() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let names = crate::dat::names::Names::load(&index, crate::dat::items::Lang::En).ok().map(|n| std::sync::Arc::new(n) as std::sync::Arc<dyn crate::dat::names::NameSource>);
        let d = Dialog::load(&index, 248).unwrap();
        let e = expand(&d.raw(6865).unwrap(), &Ctx { num: [2825, 0, 0, 0, 0, 0, 0, 0], names, ..Default::default() });
        assert_eq!(crate::dat::font::strip_codes(&crate::dat::font::decode_sjis(&e.lines[0])), "A Gilgamesh's introductory letter would be fine, too...if you can find one.");
    }

    /// East Ronfaure 7906 (a field manual page): `7f 92 00 [/s]` by num[0].
    #[test]
    fn plural_by_num() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let d = Dialog::load(&index, 101).unwrap();
        let line = |n| crate::dat::font::decode_sjis(&expand(&d.raw(7906).unwrap(), &Ctx { num: [n, 0, 0, 0, 0, 0, 0, 0], ..Default::default() }).lines[0]);
        assert_eq!(line(6), "6 members of the pugil family.");
        assert_eq!(line(1), "1 member of the pugil family.");
    }

    #[test]
    fn readable_codes() {
        let s = b"Care to learn?\x07\x0bSure!\x07No.\x7f\x31\x00\x07";
        assert_eq!(readable(s), "Care to learn?\n{choice}Sure!\nNo.{next}");
        let s = b"three \x01\x05\x25\x82\x80\x80\x80, \x7f\x85[sir/madam].";
        assert_eq!(readable(s), "three {item pl num0}, {player m/f}[sir/madam].");
    }

    #[test]
    fn expand_choice() {
        let s = b"Hi, \x08.\x07\x0bYes\x07No\x7f\x31\x00";
        let e = expand(s, &Ctx { player: "Lenon".into(), ..Default::default() });
        assert_eq!(e.lines, vec![b"Hi, Lenon.".to_vec(), b"Yes".to_vec(), b"No".to_vec()]);
        assert_eq!(e.choice, Some(1));
        assert!(e.prompt);
    }

    /// Narration lines carry the window place: `02 80 00` x 80, `03 54 01` y 340.
    #[test]
    fn place_codes() {
        let e = expand(b"\x02\x50\x00\x03\x54\x01The fortress\x7f\x34\x09\x7f\x31", &Ctx::default());
        assert_eq!(e.place, [Some(80), Some(340)]);
        assert_eq!(e.lines, vec![b"The fortress".to_vec()]);
        assert_eq!(e.wait, Some(540));
    }

    #[test]
    fn file_ids() {
        assert_eq!(file_id(230), 6650);
        assert_eq!(file_id(256), 85591);
        assert_eq!(file_id(1000), 57945);
        assert_eq!(file_id(2000), 68511);
    }

    struct Stub;

    impl Texts for Stub {
        fn kind(&self, kind: u8, id: u32) -> Option<Vec<u8>> {
            Some(format!("k{kind}:{id}").into_bytes())
        }

        fn point(&self, sub: u8, id: u16) -> Option<Vec<u8>> {
            Some(format!("p{sub:x}:{id}").into_bytes())
        }

        fn monstrosity(&self, row: i16) -> Option<Vec<u8>> {
            Some(format!("m{row}").into_bytes())
        }
    }

    fn line(zone: u16, id: u32, ctx: &Ctx) -> Option<String> {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).ok()?;
        let e = expand(&Dialog::load(&index, zone).ok()?.raw(id)?, ctx);
        Some(e.lines.iter().map(|l| crate::dat::font::strip_codes(&decode_sjis(l))).collect::<Vec<_>>().join("|"))
    }

    /// FUN_100c1650's other codes on real lines: 7f b5 (zone 71), 7f b4 (280), 7f 94 (1), 18 (1), 7f 93 (43), 7f 95 /
    /// 96 (210), and 0e's sound (102).
    #[test]
    fn more_codes() {
        let texts = Some(std::sync::Arc::new(Stub) as std::sync::Arc<dyn Texts>);
        let ctx = Ctx { num: [3, 12, 0, 0, 0, 0, 0, 0], work: [1234567, 0, 0, 0, 0], texts: texts.clone(), ..Default::default() };
        let Some(t) = line(71, 11627, &ctx) else { return };
        assert_eq!(t, "m3 (lv. 12)");
        assert_eq!(line(280, 7592, &ctx).unwrap(), "GIL:1,234,567 gil");
        let t = line(1, 70, &Ctx { num: [5, 0, 12, 0, 0, 0, 0, 7], ..Default::default() }).unwrap();
        assert!(t.contains("05") && t.contains("12") && t.contains("07"), "{t}");
        let party = vec![(0, 0x100, 0, "Me".into()), (3, 0x200, 1, "Ally".into())];
        let ctx = Ctx { party, me: 0x100, msg_name: "Kupipi".into(), num: [10, 0, 0, 0, 0, 0, 0, 0], ..Default::default() };
        assert_eq!(line(1, 22, &ctx).unwrap(), "The synergy furnace is currently in use by Ally.");
        assert!(line(43, 7418, &ctx).unwrap().starts_with("Your ally, Kupipi, has obtained 10 Petras!"));
        let t = line(210, 7492, &Ctx { num: [0x2d, 0, 0, 0, 0, 0, 0, 0], ..Default::default() }).unwrap();
        assert!(t.ends_with("$2d|Base-2 (binary): %101101"), "{t}");
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).unwrap();
        let e = expand(&Dialog::load(&index, 102).unwrap().raw(12608).unwrap(), &Ctx { num: [0, 41, 0, 0, 0, 0, 0, 0], ..Default::default() });
        assert_eq!(e.sounds, [41]);
    }
}
