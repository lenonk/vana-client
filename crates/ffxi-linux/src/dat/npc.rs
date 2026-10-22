//! NPC and monster models. 0x00E SubKind 0 carries a model id naming one self-contained DAT
//! (skeleton, meshes, textures, motions); SubKind 1/7 carry a GrapIDTbl for the character path.

use super::{DatIndex, chunks};
use super::character::{Character, Look, load_character, load_file};
use anyhow::{Result, bail};

/// DAT file of a model id: FUN_100c3180 type 0 (client entity kind 2).
pub fn model_file(model: u16) -> u32 {
    let m = model as u32;
    match m {
        0..1500 => 1300 + m,
        1500..3000 => 51795 + (m - 1500),
        3000..3500 => 96907 + m,
        _ => 98239 + m,
    }
}

/// Look as LSB stores it and 0x00E sends it from offset 0x30.
#[derive(Clone, Copy, Debug)]
pub enum NpcLook {
    /// SubKind 0 (also 5/6 put a model id here).
    Model(u16),
    /// SubKind 1 (u8 face, u8 race, slot ids) and 7 (chocobo: race 32..36 in the first byte).
    Equipped(Look),
}

/// 20 bytes: u16 SubKind, then a model id or u8 face, u8 race and 8 u16 slot ids (slot nibble ignored).
pub fn parse_look(b: &[u8]) -> Result<NpcLook> {
    if b.len() < 20 {
        bail!("look is {} bytes, want 20", b.len());
    }
    let w = |i: usize| u16::from_le_bytes([b[i * 2], b[i * 2 + 1]]);
    let id = |i: usize| w(i) & 0xFFF;
    Ok(match w(0) & 7 {
        0 | 5 | 6 => NpcLook::Model(w(1)),
        1 => NpcLook::Equipped(Look { race: b[3], face: b[2] as u16, head: id(2), body: id(3), hands: id(4), legs: id(5), feet: id(6), main: id(7), sub: id(8) }),
        7 => NpcLook::Equipped(Look { race: b[2], face: 0, head: id(2), body: id(3), hands: id(4), legs: id(5), feet: id(6), main: 0, sub: 0 }),
        k => bail!("look kind {k} (door/elevator/ship) has no model"),
    })
}

/// What a model file's "init" scheduler (chunk 0x07) sets on the actor. Scheduler data: u32 @16 =
/// offset of the record stream, records {u8 opcode, u8 length in u32s, .., u32 value @8}; opcode 0
/// ends it. In FUN_10057000, 0x8d..0x95 call the actor's race, face, head .. sub setters (vtable
/// 0xd8..0xf8; 0xd8 writes race @0xef) and 0x98 sets GrapIDTbl[0], the model id (vtable 0x104).
/// 0x8c/0x79/0xa5/0xa4 pick the idle/battle/walk/run motion set (see `MotionNames`).
#[derive(Clone, Debug, Default)]
pub struct InitScheduler {
    pub look: Option<Look>,
    pub model: Option<u16>,
    /// (op, set) in scheduler order.
    pub motion_ops: Vec<(u8, u32)>,
}

pub fn init_scheduler(file: &[u8]) -> InitScheduler {
    let mut out = InitScheduler::default();
    let Ok(chunks) = chunks(file) else { return out };
    for c in chunks.into_iter().filter(|c| c.kind == 0x07 && c.name() == "init") {
        let d = c.data;
        let Some(mut at) = d.get(16..20).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize) else { continue };
        let mut v = [None; 9];
        while at + 2 <= d.len() {
            let (op, len) = (d[at], d[at + 1] as usize * 4);
            if op == 0 || len == 0 || at + len > d.len() {
                break;
            }
            let value = (len >= 12).then(|| u32::from_le_bytes(d[at + 8..at + 12].try_into().unwrap()));
            match op {
                0x8d..=0x95 => v[(op - 0x8d) as usize] = value,
                0x98 => out.model = value.map(|m| m as u16),
                0x79 | 0x8c | 0xa4 | 0xa5 => out.motion_ops.extend(value.map(|s| (op, s))),
                _ => {}
            }
            at += len;
        }
        if let Some(race) = v[0] {
            let s = |i: usize| v[i].unwrap_or(0) as u16;
            out.look = Some(Look { race: race as u8, face: s(1), head: s(2), body: s(3), hands: s(4), legs: s(5), feet: s(6), main: s(7), sub: s(8) });
        }
    }
    out
}

pub fn load_model(index: &DatIndex, model: u16) -> Result<Character> {
    load_model_depth(index, model, 0)
}

fn load_model_depth(index: &DatIndex, model: u16, depth: u32) -> Result<Character> {
    let id = model_file(model);
    let file = index.read(id)?;
    let f = load_file(&file)?;
    let Some(skeleton) = f.skeleton else {
        // Skeleton-less ids script either a PC look or another model id (a variant whose own meshes,
        // e.g. weapons, are added to the base model; the merge is verified visually only).
        let init = init_scheduler(&file);
        if let Some(base) = init.model.filter(|&b| b != model && depth < 4) {
            let mut ch = load_model_depth(index, base, depth + 1)?;
            prefer_size_info(&mut ch, f.info);
            ch.log.insert(0, format!("model {model} -> file {id}: init scheduler sets model {base}; adding {} mesh copies, {} textures, {} animations", f.parts.len(), f.textures.len(), f.animations.len()));
            ch.log.extend(f.warnings);
            ch.parts.extend(f.parts);
            ch.textures.extend(f.textures);
            // The variant's motions and schedulers first so they win by name.
            ch.animations.splice(0..0, f.animations);
            ch.schedulers.splice(0..0, f.schedulers);
            apply_motion_ops(&mut ch, &init);
            return Ok(ch);
        }
        let Some(look) = init.look else { bail!("model {model}: file {id} has no skeleton, PC look or base model") };
        let mut ch = load_character(index, &look)?;
        prefer_size_info(&mut ch, f.info);
        apply_motion_ops(&mut ch, &init);
        ch.log.insert(0, format!("model {model} -> file {id}: init scheduler sets {look:?}"));
        return Ok(ch);
    };
    let mut log = vec![format!(
        "model {model} -> file {id}: skeleton {} ({} bones), {} mesh copies, {} textures, {} animations",
        skeleton.name,
        skeleton.bones.len(),
        f.parts.len(),
        f.textures.len(),
        f.animations.len()
    )];
    log.extend(f.warnings);
    let mut ch = Character { skeleton, parts: f.parts, textures: f.textures, animations: f.animations, size_info: [0xFF; 4], motions: Default::default(), schedulers: f.schedulers, pc: None, log, packed: None };
    ch.merge_size_info(f.info);
    apply_motion_ops(&mut ch, &init_scheduler(&file));
    Ok(ch)
}

fn apply_motion_ops(ch: &mut Character, init: &InitScheduler) {
    for &(op, set) in &init.motion_ops {
        ch.motions.apply(op, set);
    }
}

/// The model-id file loads first, so its info bytes win over those of the files it pulls in.
fn prefer_size_info(ch: &mut Character, info: Option<[u8; 16]>) {
    let later = ch.size_info;
    ch.size_info = [0xFF; 4];
    ch.merge_size_info(info);
    for (dst, src) in ch.size_info.iter_mut().zip(later) {
        if *dst == 0xFF {
            *dst = src;
        }
    }
}

pub fn load_npc(index: &DatIndex, look: &NpcLook) -> Result<Character> {
    match look {
        NpcLook::Model(m) => load_model(index, *m),
        NpcLook::Equipped(l) => load_character(index, l),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks() {
        let hex = |s: &str| (0..20).map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap()).collect::<Vec<u8>>();
        assert!(matches!(parse_look(&hex("00008e0000000000000000000000000000000000")).unwrap(), NpcLook::Model(142)));
        let NpcLook::Equipped(l) = parse_look(&hex("010002030c100c200c300c400c50006000700000")).unwrap() else { panic!() };
        assert_eq!((l.race, l.face, l.head, l.feet, l.main), (3, 2, 12, 12, 0));
        let NpcLook::Equipped(c) = parse_look(&hex("0700200000000000000000000000000000000000")).unwrap() else { panic!() };
        assert_eq!(c.race, 32);
    }

    #[test]
    fn size_info_order() {
        let skel = super::super::character::Skeleton { name: String::new(), bones: Vec::new(), points: Vec::new() };
        let mut ch = Character { skeleton: skel, parts: Vec::new(), textures: Vec::new(), animations: Vec::new(), size_info: [0xFF; 4], motions: Default::default(), schedulers: Vec::new(), pc: None, log: Vec::new(), packed: None };
        let mut later = [0xFFu8; 16];
        later[10..14].copy_from_slice(&[85, 100, 115, 140]);
        ch.merge_size_info(Some(later));
        let mut first = [0xFFu8; 16];
        first[11] = 50;
        prefer_size_info(&mut ch, Some(first));
        assert_eq!([0, 1, 2, 3].map(|s| ch.scale(s)), [0.85, 0.5, 1.15, 1.4]);
    }

    #[test]
    fn model_ranges() {
        assert_eq!(model_file(268), 1568);
        assert_eq!(model_file(1500), 51795);
        assert_eq!(model_file(2920), 53215);
        assert_eq!(model_file(3000), 99907);
        assert_eq!(model_file(3500), 101739);
    }
}
