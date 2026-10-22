//! Zone event scripts (FUN_100ad140 loads, FUN_100ad460 binds blocks to actors, FUN_100bb300 builds
//! the per-actor VM, FUN_100ba3f0 dispatches opcodes) and their disassembly.

use super::DatIndex;
use anyhow::{Context, Result, ensure};

/// Event DAT file id of a zone (FUN_100ad140).
pub fn file_id(zone: u16) -> u32 {
    let z = zone as u32;
    match zone {
        0..=0xff => z + 0x16bc,
        0x100..=999 => z + 0x14aff,
        1000..=1999 => z + 0xde31,
        _ => z + 0x1004b,
    }
}

/// One actor's block: entry points per event id, constants and bytecode.
pub struct Block {
    /// Server id (0x01000000 | zone << 12 | index) or a special id (0x7ffffff0 = the player).
    pub actor: u32,
    /// (event id, code offset); id 0xffff is the actor's default (idle) entry, 0xfffe matches any event.
    pub events: Vec<(u16, u16)>,
    pub consts: Vec<u32>,
    pub code: Vec<u8>,
}

/// Header: u32 count (low 30 bits), count x u32 block sizes, then the blocks (FUN_100ad460).
/// Block: u32 actor, u32 n, u16 offsets[n], u16 ids[n], u32 nconst, u32 consts[nconst], u32 code length, code.
pub fn parse(file: &[u8]) -> Result<Vec<Block>> {
    let u32_at = |b: &[u8], o: usize| -> Result<u32> { Ok(u32::from_le_bytes(b.get(o..o + 4).context("event data truncated")?.try_into().unwrap())) };
    let u16_at = |b: &[u8], o: usize| -> Result<u16> { Ok(u16::from_le_bytes(b.get(o..o + 2).context("event data truncated")?.try_into().unwrap())) };
    let n = (u32_at(file, 0)? & 0x3fff_ffff) as usize;
    let mut p = 4 + 4 * n;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let size = u32_at(file, 4 + 4 * i)? as usize;
        let b = file.get(p..p + size).context("block past end")?;
        p += size;
        let (actor, ne) = (u32_at(b, 0)?, u16_at(b, 4)? as usize);
        let events = (0..ne).map(|k| Ok((u16_at(b, 8 + 2 * ne + 2 * k)?, u16_at(b, 8 + 2 * k)?))).collect::<Result<Vec<_>>>()?;
        let q = 8 + 4 * ne;
        let nc = u16_at(b, q)? as usize;
        let consts = (0..nc).map(|k| u32_at(b, q + 4 + 4 * k)).collect::<Result<Vec<_>>>()?;
        let len = u32_at(b, q + 4 + 4 * nc)? as usize;
        let start = q + 8 + 4 * nc;
        let code = b.get(start..start + len).context("code past end")?.to_vec();
        out.push(Block { actor, events, consts, code });
    }
    ensure!(p == file.len(), "event data: {} trailing bytes", file.len() as isize - p as isize);
    Ok(out)
}

pub fn load(index: &DatIndex, zone: u16) -> Result<Vec<Block>> {
    parse(&index.read(file_id(zone))?).with_context(|| format!("event file of zone {zone}"))
}

/// Operand kinds of the opcode table (event_ops.rs).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// u16 value operand (FUN_100ada80): bit 15 = constant, < 0x800 local, 0x1000.. zone work,
    /// 0x1100.. memorize, 0x1700.., 0x7f00.. event actor, 0x7f80.. player.
    Val,
    /// u16 variable to store to (FUN_100addc0).
    Set,
    U8,
    U16,
    U32,
    /// u16 string/vector operand (FUN_100adf30).
    Str,
    /// u32 four-character motion or scheduler name.
    Motion,
    /// u32 actor: 0x7ffffff0 player, 0x7ffffff8 event actor, else a server id.
    Actor,
    /// value operand holding a dialogue message id.
    Msg,
    /// 16 raw bytes (a name).
    Lit16,
}

#[derive(Clone, Copy)]
pub enum Len {
    Fixed(u8),
    /// The byte at pc + by selects the length; 0 = the handler does not advance (stall).
    Sub { by: u8, map: &'static [(u8, u8)], default: u8 },
}

type Args = &'static [(Kind, i8)];

#[derive(Clone, Copy)]
pub struct Op {
    pub name: &'static str,
    pub len: Len,
    pub args: Args,
    /// Per sub-opcode operands (selector = the Len::Sub byte).
    pub sub_args: &'static [(u8, Args)],
    pub jumps: &'static [u8],
    pub sub_jumps: &'static [(u8, &'static [u8])],
    /// May yield (wait) before advancing.
    pub waits: bool,
    /// No fall-through (end, goto, return, event end, halt).
    pub stop: bool,
    pub desc: &'static str,
}

pub fn op_at(code: &[u8], pc: usize) -> Option<(&'static Op, usize, Option<u8>)> {
    let op = super::event_ops::OPS[*code.get(pc)? as usize].as_ref()?;
    match op.len {
        Len::Fixed(n) => Some((op, n as usize, None)),
        Len::Sub { by, map, default } => {
            let s = *code.get(pc + by as usize)?;
            let n = map.iter().find(|(k, _)| *k == s).map(|(_, v)| *v).unwrap_or(default);
            (n > 0).then_some((op, n as usize, Some(s)))
        }
    }
}

fn val_name(v: u16, consts: &[u32]) -> String {
    match v {
        _ if v & 0x8000 != 0 => match consts.get((v & 0x7fff) as usize) {
            Some(c) => format!("#{}", *c as i32),
            None => format!("#?{v:04x}"),
        },
        0..=0x7ff => format!("L{v}"),
        0x1000..=0x10ff => format!("Z{}", v - 0x1000),
        0x1100..=0x11ff => format!("M{}", v - 0x1100),
        0x1700..=0x17ff => format!("G{}", v - 0x1700),
        0x7f00..=0x7f7f => format!("self.{:x}", v - 0x7f00),
        0x7f80..=0x7fff => format!("player.{:x}", v - 0x7f80),
        _ => format!("v{v:04x}"),
    }
}

fn actor_name(a: u32) -> String {
    match a {
        0x7fff_fff0 => "player".into(),
        0x7fff_fff8 => "self".into(),
        _ if a & 0xff00_0000 == 0x0100_0000 => format!("npc{:03x}", a & 0xfff),
        _ => format!("{a:08x}"),
    }
}

/// Instructions reached from the event entries and their jump targets (the code holds inline data
/// tables, so a linear sweep is wrong): (pc, op, length, selector).
pub fn walk(b: &Block) -> (Vec<(usize, &'static Op, usize, Option<u8>)>, Vec<usize>) {
    let code = &b.code;
    let mut seen = std::collections::BTreeMap::new();
    let mut bad = Vec::new();
    let mut work: Vec<usize> = b.events.iter().map(|(_, at)| *at as usize).filter(|&a| a < code.len()).collect();
    while let Some(mut pc) = work.pop() {
        while pc < code.len() && !seen.contains_key(&pc) {
            let Some((op, n, sub)) = op_at(code, pc) else {
                bad.push(pc);
                break;
            };
            seen.insert(pc, (op, n, sub));
            let jumps = match sub {
                Some(s) if !op.sub_jumps.is_empty() => op.sub_jumps.iter().find(|(k, _)| *k == s).map(|(_, j)| *j).unwrap_or(&[]),
                _ => op.jumps,
            };
            for &j in jumps {
                if let Some(t) = code.get(pc + j as usize..pc + j as usize + 2) {
                    let t = u16::from_le_bytes([t[0], t[1]]) as usize;
                    if t < code.len() {
                        work.push(t);
                    }
                }
            }
            if op.stop {
                break;
            }
            pc += n;
        }
    }
    (seen.into_iter().map(|(pc, (op, n, s))| (pc, op, n, s)).collect(), bad)
}

/// Disassembly listing; `text` resolves a dialogue message id. Unreached bytes are shown as data.
pub fn disasm(b: &Block, text: &dyn Fn(u32) -> Option<String>) -> Vec<String> {
    let code = &b.code;
    let (ins, bad) = walk(b);
    let mut out = Vec::new();
    let mut last = 0;
    for (pc, op, n, sub) in ins {
        if pc > last {
            out.push(format!("  {last:04x}: data {}", code[last..pc].iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")));
        }
        last = pc + n;
        for (id, at) in &b.events {
            if *at as usize == pc {
                out.push(format!("event_{id:04x}:"));
            }
        }
        let raw = &code[pc..(pc + n).min(code.len())];
        let at = |o: i8, w: usize| -> Option<&[u8]> {
            let s = pc as isize + o as isize;
            if s < 0 { None } else { code.get(s as usize..s as usize + w) }
        };
        let u16a = |o: i8| at(o, 2).map(|x| u16::from_le_bytes([x[0], x[1]])).unwrap_or(0);
        let u32a = |o: i8| at(o, 4).map(|x| u32::from_le_bytes(x.try_into().unwrap())).unwrap_or(0);
        let args = match sub {
            Some(s) if !op.sub_args.is_empty() => op.sub_args.iter().find(|(k, _)| *k == s).map(|(_, a)| *a).unwrap_or(op.args),
            _ => op.args,
        };
        let jumps = match sub {
            Some(s) if !op.sub_jumps.is_empty() => op.sub_jumps.iter().find(|(k, _)| *k == s).map(|(_, j)| *j).unwrap_or(&[]),
            _ => op.jumps,
        };
        let mut parts = Vec::new();
        let mut note = String::new();
        if let Some(s) = sub {
            parts.push(format!("sub {s}"));
        }
        for &(k, o) in args {
            if k == Kind::U16 && jumps.iter().any(|&j| j as i8 == o) {
                continue;
            }
            parts.push(match k {
                Kind::Val | Kind::Set | Kind::Str => val_name(u16a(o), &b.consts),
                Kind::Msg => {
                    let v = u16a(o);
                    if v & 0x8000 != 0 {
                        if let Some(t) = b.consts.get((v & 0x7fff) as usize).and_then(|&c| text(c)) {
                            note = format!("  \"{t}\"");
                        }
                    }
                    format!("msg {}", val_name(v, &b.consts))
                }
                Kind::U8 => format!("{}", at(o, 1).map(|x| x[0]).unwrap_or(0)),
                Kind::U16 => format!("{:#x}", u16a(o)),
                Kind::U32 => format!("{:#x}", u32a(o)),
                Kind::Motion => format!("'{}'", String::from_utf8_lossy(&u32a(o).to_le_bytes()).trim_end_matches('\0')),
                Kind::Actor => actor_name(u32a(o)),
                Kind::Lit16 => format!("\"{}\"", String::from_utf8_lossy(at(o, 16).unwrap_or(&[])).trim_end_matches('\0')),
            });
        }
        for &j in jumps {
            parts.push(format!("->{:04x}", u16a(j as i8)));
        }
        let hex = raw.iter().take(12).map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ");
        out.push(format!("  {pc:04x}: {hex:<36} {}{}{}", op.name, if parts.is_empty() { String::new() } else { format!(" {}", parts.join(", ")) }, note));
    }
    if last < code.len() {
        out.push(format!("  {last:04x}: data ({} bytes)", code.len() - last));
    }
    for pc in bad {
        out.push(format!("  ; stopped at {pc:04x}: opcode {:02x} has no length for this form", code[pc]));
    }
    out
}
