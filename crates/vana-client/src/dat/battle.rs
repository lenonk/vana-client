//! Battle messages ("msg basic"), as 0x029 / 0x02D (FUN_1009cf70 / FUN_1009d1b0) and 0x028 results
//! (FUN_1009c730) print them: text FUN_10094d20(5, id) from the 1024-entry file in language slot 0x63,
//! expanded by FUN_100bf790 with the caster / target names, and a chat mode per caster-target relation
//! (FUN_1009cda0) that picks the line colour (FUN_10126010 / FUN_10126040).

use super::DatIndex;
use super::dialog::Dialog;
use super::font::{decode_sjis, encode_sjis};
use super::msg::{Messages, slot_file};
use anyhow::{Context, Result};

/// Language slots (FUN_10250e50): message kinds of FUN_10094d20 and the name tables.
const SLOT_BASIC: usize = 0x63; // kind 5
const SLOT_SKILLS: usize = 0x60; // kind 2: combat skills ({05})
const SLOT_LISTS: usize = 0x61; // kind 3: 7f 84 bit names
const SLOT_MOB_SKILLS: usize = 0x66; // kind 7: {16} ids from 0x100
pub(crate) const SLOT_SPELLS: usize = 0x5a; // FUN_10221f40
pub(crate) const SLOT_ABILITIES: usize = 0x5c; // FUN_10219c20
const SLOT_SYSTEM: usize = 0x65; // kind 6: s2c 0x053 (FUN_10094d20(6, n))
const SLOT_EMOTES: usize = 0x62; // s2c 0x05A's lines (LAB_100a06e0)
/// Status names (grammar kind 0x13, FUN_1021d5f0); file inferred from content, slot not traced.
const STATUS_FILE: u32 = 55725;

// Mode tables (1024 bytes each, by message id, FUN_1009cda0's order), mode -> colour id {u16 mode, u8 colour,
// u8 group} (zero-terminated), colour id -> BGRA {u32 id, u32 colour}: located by dat::tables.
/// FUN_1009cda0 returns 0xC8 for lines that are not shown.
pub const HIDDEN: u8 = 0xC8;

/// Who a message is about, relative to us (FUN_1009cda0's tests, first match wins).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    TargetIsMe,
    CasterIsMe,
    PartyCaster,
    PartyTarget,
    AllianceCaster,
    AllianceTarget,
    Other,
}

/// An actor as the expander sees it: entity +0x7c name, id class, and the 0x00D/0x00E name flags.
#[derive(Clone, Debug, Default)]
pub struct Who {
    pub name: String,
    /// Player ids are below 0x01000000; they take no article.
    pub pc: bool,
    /// u32 @0x24 bit 29 (FUN_100892d0): no "the" (proper noun).
    pub named: bool,
    /// u32 @0x24 bit 30 (FUN_10089320): plural verb forms.
    pub plural: bool,
    /// 0x00D u32 @0x20 bit 15 clear (our own: the race): [his/her] (7f 90 / 91).
    pub female: bool,
}

/// What the codes read: the two actors, our own name and work[5] (message object +0xc50).
#[derive(Clone, Debug, Default)]
pub struct Ctx {
    pub actor: Option<Who>,
    pub target: Option<Who>,
    pub me: String,
    pub work: [i32; 5],
    /// String slots 2 / 3 of the queue entry (+0xd04; 0 / 1 are the actor and target names): FUN_100a1ab0's strings.
    pub strings: [String; 2],
}

#[derive(Debug, PartialEq)]
pub struct Line {
    pub lines: Vec<String>,
    /// A `1f xx` code sets the message mode (msg+4).
    pub mode: Option<u8>,
}

pub struct BattleText {
    basic: Dialog,
    skills: Option<Dialog>,
    lists: Option<Dialog>,
    mob_skills: Option<Dialog>,
    spells: Option<Messages>,
    abilities: Option<Messages>,
    statuses: Option<Messages>,
    items: Option<super::items::Items>,
    /// File 7031 (language slot 0x65): s2c 0x053 system messages; group 9 (file 55648) client messages.
    system: Option<Dialog>,
    group7: Option<Messages>,
    /// Emote lines (base slot 0x62, file 7025): entry emote * 2 with a target, + 1 without.
    emotes: Option<Dialog>,
    /// The auto-translate dictionary (language slot 0x18).
    pub phrases: Option<super::autotrans::Dictionary>,
    /// Group 0x17: equipment slot names for /equip ("main" .. "feet").
    slot_names: Option<Messages>,
    group9: Option<Messages>,
    /// Group 10: formatted client lines (the guild shop's hours, ...); group 0x10: the weekday names.
    group10: Option<Messages>,
    /// Group 0xc: menu and command lines ("Set #%d equipped.", ...).
    group12: Option<Messages>,
    days: Option<Messages>,
    /// Group 0x11: the moon phase names.
    moons: Option<Messages>,
    /// Grammar kinds 0x87 / 0x88 / 0x8a (FUN_100bdc50 cases 6 / 7 / 9): expander tables +0x788 / +0x858 / +0x8c0, this
    /// build's language slots 0x1c / 0x22 / 0x1b (0x22: trust party lines, s2c 0x02D message 711).
    tables: [Option<Messages>; 3],
    modes: Vec<[u8; 1024]>,
    mode_colours: Vec<(u16, u8)>,
    palette: Vec<(u8, [u8; 4])>,
    /// Chat filter (FUN_100abf20 / FUN_100ac450): kind per message (0x10350a60) and 11 relation tables of 49
    /// words (pointers at 0x10351bbc): top 2 bits pick config 0x32 / 0x33, the low 30 the bits.
    filter_kinds: Vec<u8>,
    filter_tables: Vec<Vec<u32>>,
    /// On-screen kind per message (0x1035cae0): 0 none, 1 damage (`damv`), 2 recovery (`curv`), 3 miss (`misv`).
    screen_kinds: Vec<u8>,
    /// Chat call tags and their sounds (DAT_1036eba0).
    pub calls: Vec<(String, u32)>,
}

/// Config id -> colour id of the log palette (FUN_101262c0).
pub fn colour_id_of(config: u16) -> Option<u8> {
    match config {
        0x1b..=0x31 => Some((config - 0x1b) as u8 + 0x54),
        0x9b => Some(0x6c),
        0xb0 => Some(0x6d),
        0xb3 => Some(0x6e),
        // FUN_10129f80: the assist channels (J / E).
        0xcb => Some(0x70),
        0xcc => Some(0x71),
        _ => None,
    }
}

/// A config colour 0x80RRGGBB as glyph BGRA bytes.
pub fn argb_bytes(c: u32) -> [u8; 4] {
    let b = c.to_le_bytes();
    [b[0], b[1], b[2], b[3]]
}

/// Bytes at a virtual address of a PE image (data sections; FFXiMain keeps them unpacked).
pub fn va_bytes(pe: &[u8], va: u32, len: usize) -> Option<&[u8]> {
    let u16_at = |o: usize| pe.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let u32_at = |o: usize| pe.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let nt = u32_at(0x3c)? as usize;
    let (count, opt) = (u16_at(nt + 6)?, u16_at(nt + 20)?);
    let base = u32_at(nt + 24 + 28)?;
    (0..count).find_map(|i| {
        let s = nt + 24 + opt + i * 40;
        let (sva, raw, rsize) = (base + u32_at(s + 12)?, u32_at(s + 20)?, u32_at(s + 16)?);
        (va >= sva && va + len as u32 <= sva + rsize).then(|| pe.get((raw + va - sva) as usize..(raw + va - sva) as usize + len)).flatten()
    })
}

impl BattleText {
    pub fn load(index: &DatIndex, ffximain: &[u8]) -> Result<Self> {
        let file = |slot| slot_file(ffximain, 1, slot).with_context(|| format!("language slot {slot:#x}"));
        let dialog = |slot| -> Result<Dialog> { Dialog::parse(&index.read(file(slot)?)?) };
        let msgs = |id: u32| Messages::load(index, id).map_err(|e| log::warn!("names {id}: {e:#}")).ok();
        let basic = dialog(SLOT_BASIC).context("battle messages")?;
        let t = super::tables::of(ffximain)?;
        let modes = t.mode_tables.iter().map(|&va| va_bytes(ffximain, va, 1024).and_then(|b| b.try_into().ok()).context("mode table")).collect::<Result<Vec<[u8; 1024]>>>()?;
        let mut mode_colours = Vec::new();
        let mut at = t.mode_colours;
        while let Some(e) = va_bytes(ffximain, at, 4).filter(|e| e[0] | e[1] != 0) {
            mode_colours.push((u16::from_le_bytes([e[0], e[1]]), e[2]));
            at += 4;
        }
        let mut palette = Vec::new();
        let mut at = t.palette;
        while let Some(e) = va_bytes(ffximain, at, 8).filter(|e| e[0] != 0) {
            palette.push((e[0], [e[4], e[5], e[6], e[7]]));
            at += 8;
        }
        let filter_kinds = va_bytes(ffximain, t.filter_kinds, 1024).map(<[u8]>::to_vec).unwrap_or_default();
        let screen_kinds = va_bytes(ffximain, t.screen_kinds, 1024).map(<[u8]>::to_vec).unwrap_or_default();
        let calls = super::tables::call_sounds(ffximain).unwrap_or_default();
        let filter_tables = t
            .filter_tables
            .iter()
            .filter_map(|&p| Some(va_bytes(ffximain, p, 49 * 4)?.chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect()))
            .collect();
        Ok(Self {
            filter_kinds,
            filter_tables,
            screen_kinds,
            calls,
            basic,
            skills: dialog(SLOT_SKILLS).ok(),
            lists: dialog(SLOT_LISTS).ok(),
            mob_skills: dialog(SLOT_MOB_SKILLS).ok(),
            spells: file(SLOT_SPELLS).ok().and_then(msgs),
            abilities: file(SLOT_ABILITIES).ok().and_then(msgs),
            statuses: msgs(STATUS_FILE),
            items: super::items::Items::load(index, super::items::Lang::En).ok(),
            system: dialog(SLOT_SYSTEM).map_err(|e| log::warn!("system messages: {e:#}")).ok(),
            group7: super::msg::group_file(ffximain, 1, 7).and_then(msgs),
            emotes: dialog(SLOT_EMOTES).map_err(|e| log::warn!("emote messages: {e:#}")).ok(),
            slot_names: super::msg::group_file(ffximain, 1, 0x17).and_then(msgs),
            group9: super::msg::group_file(ffximain, 1, 9).and_then(msgs),
            group10: super::msg::group_file(ffximain, 1, 10).and_then(msgs),
            group12: super::msg::group_file(ffximain, 1, 0xc).and_then(msgs),
            days: super::msg::group_file(ffximain, 1, 0x10).and_then(msgs),
            moons: super::msg::group_file(ffximain, 1, 0x11).and_then(msgs),
            tables: [0x1c, 0x22, 0x1b].map(|slot| super::msg::current_slot_file(ffximain, 1, slot).and_then(msgs)),
            phrases: super::autotrans::Dictionary::load(index, ffximain).map_err(|e| log::warn!("auto-translate: {e:#}")).ok(),
            modes,
            mode_colours,
            palette,
        })
    }

    /// The chat mode of a message for a relation; `HIDDEN` lines are not printed.
    pub fn mode(&self, id: u16, rel: Relation) -> u8 {
        let t = match rel {
            Relation::TargetIsMe => 0,
            Relation::CasterIsMe => 1,
            Relation::PartyCaster => 2,
            Relation::PartyTarget => 3,
            Relation::AllianceCaster => 4,
            Relation::AllianceTarget => 5,
            Relation::Other => 6,
        };
        self.modes[t].get(id as usize).copied().unwrap_or(HIDDEN)
    }

    /// Line colour of a chat mode, BGRA with 0x80 = 1.0; 0x80808080 when unlisted (FUN_10126040).
    pub fn colour(&self, mode: u8) -> [u8; 4] {
        let id = self.mode_colours.iter().find(|(m, _)| *m == mode as u16).map_or(0, |e| e.1);
        self.palette.iter().find(|(i, _)| *i == id).map_or([0x80; 4], |e| e.1)
    }

    /// The line colour with the config's text colours (callback LAB_10126560 replaces the palette entries).
    pub fn colour_in(&self, mode: u8, cfg: &crate::gamecfg::Values) -> [u8; 4] {
        let id = self.mode_colours.iter().find(|(m, _)| *m == mode as u16).map_or(0, |e| e.1);
        let config = (0x1b..=0x31).chain([0x9b, 0xb0, 0xb3, 0xcb, 0xcc]).find(|&c| colour_id_of(c) == Some(id));
        match config {
            Some(c) => argb_bytes(cfg.get(c)),
            None => self.palette.iter().find(|(i, _)| *i == id).map_or([0x80; 4], |e| e.1),
        }
    }

    /// FUN_100ac450: whether message `id` is filtered for relation table `rel` (0 caster us, 1 target us, 2 / 3
    /// party caster / target, 4 / 5 alliance, 6 / 7 our mob, 8 / 9 other PC, 10 else) under 0x32 / 0x33.
    pub fn filtered(&self, id: u16, rel: usize, cfg: &crate::gamecfg::Values) -> bool {
        let Some(&kind) = self.filter_kinds.get(id as usize) else { return false };
        let Some(&w) = self.filter_tables.get(rel).and_then(|t| t.get(kind as usize)) else { return false };
        let word = if w >> 30 == 0 { cfg.get(0x32) } else { cfg.get(0x33) };
        w & 0x3fff_ffff & word != 0
    }

    /// A status name for the title bar (FUN_10227450: file 55725 field 0 through FUN_102588c0(2); the capitals are
    /// inferred: each word's first letter, as "Haste").
    pub fn status_title(&self, id: u16) -> String {
        let mut t = Self::msg_of(&self.statuses, id as i32);
        apply_case(&mut t, 4);
        decode_sjis(&t)
    }

    /// A mob skill's name (kind 7, ids from 0x100), as the 16 n code expands it.
    pub fn mob_skill(&self, id: u16) -> String {
        if id < 0x100 {
            return String::new();
        }
        decode_sjis(&Self::name_of(&self.mob_skills, id as u32 - 0x100))
    }

    pub fn screen_kind(&self, id: u16) -> u8 {
        self.screen_kinds.get(id as usize).copied().unwrap_or(0)
    }

    /// FUN_100ad8f0 under config 0x13 (0 Log, 1 Both, 2 Screen): whether message `id` is kept off channel 0 (the
    /// log) or 1 (the screen). A message with no kind never goes on screen and always to the log.
    pub fn off_channel(&self, id: u16, channel: u8, cfg: &crate::gamecfg::Values) -> bool {
        let mode = cfg.get(0x13);
        match self.screen_kind(id) {
            0 => channel != 0,
            _ => (channel == 1 && mode == 0) || (channel == 0 && mode == 2),
        }
    }

    pub fn raw(&self, id: u16) -> Option<Vec<u8>> {
        self.basic.raw(id as u32)
    }

    fn name_of(d: &Option<Dialog>, id: u32) -> Vec<u8> {
        d.as_ref().and_then(|d| d.raw(id)).map(|r| r.split(|&b| b == 0).next().unwrap_or_default().to_vec()).unwrap_or_default()
    }

    fn msg_of(m: &Option<Messages>, id: i32) -> Vec<u8> {
        m.as_ref().and_then(|m| m.raw(id.max(0) as usize)).unwrap_or_default()
    }

    /// English item name: form 0 singular, 1 with its article, 2 plural.
    /// The noun of form 0 (bare), 1 (singular, mode 3) or 2 (plural, mode 4) and whether it takes "an".
    fn item_noun(&self, id: u32, form: u8) -> Option<(String, u8, bool)> {
        let n = super::names::item_noun(self.items.as_ref()?.item(id)?);
        Some(match form {
            0 => (n.bare, n.mode, n.an),
            1 => (n.singular, 3, n.singular_an),
            _ => (n.plural, 4, false),
        })
    }

    /// A 0x053 system message expanded for `ctx`.
    /// An emote's log line (LAB_100a06e0): entry emote * 2, + 1 without a target.
    pub fn emote(&self, entry: u32, ctx: &Ctx) -> Option<Line> {
        let raw = self.emotes.as_ref()?.raw(entry)?;
        Some(self.expand_raw(&raw, ctx))
    }

    pub fn system(&self, id: u16, ctx: &Ctx) -> Option<Line> {
        let raw = self.system.as_ref()?.raw(id as u32)?;
        Some(self.expand_raw(&raw, ctx))
    }

    /// A group 9 client message (FUN_10076e80).
    /// Group 7: command refusals (FUN_10129d90 / FUN_10129e30).
    pub fn group7(&self, id: usize) -> Option<String> {
        self.group7.as_ref()?.get(id)
    }

    /// Group 0x17 entry `id` (an /equip slot name).
    pub fn slot_name(&self, id: usize) -> Option<String> {
        self.slot_names.as_ref()?.get(id)
    }

    pub fn group9(&self, id: usize) -> Option<String> {
        self.group9.as_ref()?.get(id)
    }

    pub fn group10(&self, id: usize) -> Option<String> {
        self.group10.as_ref()?.get(id)
    }

    pub fn group12(&self, id: usize) -> Option<String> {
        self.group12.as_ref()?.get(id)
    }

    /// A group 10 string's bytes, its FA 40 codes kept (for dat::msgfmt).
    pub fn group10_raw(&self, id: usize) -> Option<Vec<u8>> {
        self.group10.as_ref()?.raw(id)
    }

    /// Group 0x11 entry `phase` (0 New Moon .. 11).
    pub fn moon_name(&self, phase: usize) -> Option<String> {
        self.moons.as_ref()?.get(phase)
    }

    /// Group 0x10 entry `day`: 0 Firesday .. 7 Darksday.
    pub fn day_name(&self, day: usize) -> Option<String> {
        self.days.as_ref()?.get(day)
    }

    /// Message `id` expanded for `ctx`; None when the id has no text.
    pub fn expand(&self, id: u16, ctx: &Ctx) -> Option<Line> {
        Some(self.expand_raw(&self.raw(id)?, ctx))
    }

    /// FUN_100bf790 (English branch) over the codes battle messages use.
    pub fn expand_raw(&self, s: &[u8], ctx: &Ctx) -> Line {
        let mut lines: Vec<Vec<u8>> = vec![Vec::new()];
        let mut mode = None;
        let work = |k: u8| ctx.work.get(k as usize).copied().unwrap_or(0);
        let who = |k: u8| if k == 0 { ctx.actor.as_ref() } else { ctx.target.as_ref() };
        let mut i = 0;
        // A count of one prints nothing, nor the space after it (FUN_1024fcd0(0) *(inf)*).
        let mut eat_space = false;
        // 7f 80 n (expander +0xc): the case of the next grammar block's text (FUN_100c2bf0).
        let mut case_mode = 0u8;
        // An article token (FUN_1025a400: kind 1 a / an, 2 the, a count of one) waits for the next noun (line, byte).
        let mut pending: Option<(usize, usize, bool)> = None;
        while i < s.len() {
            let line_no = lines.len() - 1;
            let c = s[i];
            let a = s.get(i + 1).copied().unwrap_or(0);
            let out = lines.last_mut().unwrap();
            if std::mem::take(&mut eat_space) && c == b' ' {
                i += 1;
                continue;
            }
            let step = match c {
                0x80.. => {
                    out.extend_from_slice(&s[i..(i + 2).min(s.len())]);
                    2
                }
                0x00 => break,
                0x07 => {
                    lines.push(Vec::new());
                    1
                }
                // 01 len kind args: English grammar block (FUN_100bc8a0).
                0x01 => {
                    let body = s.get(i + 2..(i + 2 + a as usize).min(s.len())).unwrap_or_default();
                    let idx = grammar_index(body.get(1..).unwrap_or_default());
                    let slot = idx as u8;
                    let mut start = out.len();
                    // FUN_1025a2f0's noun token: the waiting article goes in front (a / an for mode 3, the, some).
                    let mut noun = |out: &mut Vec<u8>, start: &mut usize, n: Option<(String, u8, bool)>| {
                        let Some((text, mode, an)) = n else { return };
                        if let Some((l, pos, definite)) = pending.take()
                            && l == line_no
                            && pos <= out.len()
                        {
                            let art = match (definite, mode) {
                                (true, _) => "the",
                                (false, 3) => if an { "an" } else { "a" },
                                (false, _) => "some",
                            };
                            out.splice(pos..pos, art.bytes());
                            *start = (*start).min(pos);
                        }
                        out.extend(encode_sjis(&text));
                    };
                    match body.first() {
                        // Kinds 1 / 2 (FUN_1025a400(0 / 1)): an article for the next noun.
                        Some(&k @ (0x01 | 0x02)) => pending = Some((line_no, out.len(), k == 0x02)),
                        Some(0x10) => out.extend(encode_sjis(&ctx.actor.as_ref().map_or(String::new(), |w| w.name.clone()))),
                        Some(0x11) => out.extend(encode_sjis(&ctx.target.as_ref().map_or(String::new(), |w| w.name.clone()))),
                        // FUN_1021d5f0 / FUN_1021d650: the status noun (field 0, "stun") / adjective (field 1, "stunned").
                        Some(0x13) => out.extend(Self::msg_of(&self.statuses, work(slot))),
                        Some(0x14) => out.extend(self.statuses.as_ref().and_then(|m| m.raw_field(work(slot).max(0) as usize, 1)).unwrap_or_default()),
                        // Kinds 3 / 4: the count, or for one the indefinite article.
                        Some(0x03 | 0x04) if work(slot) != 1 => out.extend(work(slot).to_string().bytes()),
                        Some(0x03 | 0x04) => pending = Some((line_no, out.len(), false)),
                        // Items (FUN_100be760): form (k % 3) bare / singular / plural; the id is the index itself (0x20..0x22),
                        // num (0x23..0x25) or work (0x26..0x28), num read as work here (equal for FUN_100a1ab0 lines).
                        Some(&k @ 0x20..=0x28) => {
                            let id = if k < 0x23 { idx as i32 } else { work(slot) };
                            noun(out, &mut start, self.item_noun(id as u32, (k - 0x20) % 3));
                        }
                        // 0x39 / 0x40 / 0x41: an RoE objective's name, id + 0xe000 from the index, num or work (num read
                        // as work here).
                        Some(&k @ (0x39 | 0x40 | 0x41)) => {
                            let n = if k == 0x39 { idx as i32 } else { work(slot) };
                            if let Some(o) = self.items.as_ref().and_then(|it| super::roe::objective(it, (n + 0xe000) as u16)) {
                                out.extend(encode_sjis(&o.name));
                            }
                        }
                        // FUN_100bdc50 cases 6 / 7 / 9: a line of table 0 / 1 / 2 by num / work / work (num read as work).
                        Some(&k @ (0x87 | 0x88 | 0x8a)) => out.extend(Self::msg_of(&self.tables[match k {
                            0x87 => 0,
                            0x88 => 1,
                            _ => 2,
                        }], work(slot))),
                        // 0x29 / 0x2a: a count index, then an item index; the singular for one, else the plural.
                        Some(0x29 | 0x2a) => {
                            let first = body.get(1..).unwrap_or_default();
                            let item = grammar_index(first.get(grammar_len(first)..).unwrap_or_default()) as u8;
                            let n = work(slot);
                            noun(out, &mut start, self.item_noun(work(item) as u32, if n == 1 { 1 } else { 2 }));
                        }
                        _ => {}
                    }
                    // Only a block that wrote text takes the case (FUN_100be760: FUN_10259f40 then +0xc = 0).
                    if out.len() > start {
                        apply_case(&mut out[start..], std::mem::take(&mut case_mode));
                    }
                    2 + a as usize
                }
                0x02 | 0x03 => 3,
                0x05 => {
                    out.extend(Self::name_of(&self.skills, work(a) as u16 as u32));
                    2
                }
                0x08 => {
                    out.extend(encode_sjis(&ctx.me));
                    1
                }
                0x10 => {
                    out.extend(Self::msg_of(&self.spells, work(a) as u16 as i32));
                    2
                }
                0x12 => {
                    out.extend(work(a).to_string().bytes());
                    2
                }
                0x14 => {
                    let t = work(a);
                    out.extend(format!("{}:{:02}:{:02}", t / 3600, t % 3600 / 60, t % 60).bytes());
                    2
                }
                // 16 n: an ability below 0x100 (FUN_100bba90), else kind 7 (mob skills) id - 0x100.
                0x16 => {
                    let id = work(a) as u16;
                    out.extend(if id < 0x100 { Self::msg_of(&self.abilities, id as i32) } else { Self::name_of(&self.mob_skills, id as u32 - 0x100) });
                    2
                }
                0x1f => {
                    mode = Some(a);
                    2
                }
                // 1c n (FUN_100c1650, 0x100c24f7): string slot n.
                0x1c => {
                    let s = match a {
                        0 => ctx.actor.as_ref().map(|w| w.name.clone()),
                        1 => ctx.target.as_ref().map(|w| w.name.clone()),
                        n => ctx.strings.get(n as usize - 2).cloned(),
                    };
                    out.extend(encode_sjis(&s.unwrap_or_default()));
                    2
                }
                // 0a n (0x100c24aa): num[n] as "%d" (the queue entry +0xc64; equal to work for FUN_100a1ab0 lines).
                0x0a => {
                    out.extend(work(a).to_string().bytes());
                    2
                }
                0x0d | 0x0e | 0x11 | 0x13 | 0x17..=0x19 | 0x1a | 0x1e => 2,
                0x7f => match a {
                    0x80 => {
                        case_mode = s.get(i + 2).copied().unwrap_or(0);
                        3
                    }
                    // 7f b4 n (0x100c20b9): work[n] clamped to 0..999999999 with separators (FUN_101c9440; ',' in
                    // English), then group 10 0x5f "%s gil" (English text).
                    0xb4 => {
                        let v = work(s.get(i + 2).copied().unwrap_or(0)).clamp(0, 999_999_999);
                        out.extend(format!("{} gil", separated(v as u32)).bytes());
                        3
                    }
                    0x34..=0x36 | 0x81 | 0xa0..=0xb5 => 3,
                    0x38 => 4,
                    0x84 => {
                        // Names of the set bits of work[4], kind 3 entries n * 0x20 + bit, space separated.
                        let base = s.get(i + 2).copied().unwrap_or(0) as u32 * 0x20;
                        let mut first = true;
                        for bit in (0..32).filter(|b| work(4) as u32 >> b & 1 != 0) {
                            if !first {
                                out.push(b' ');
                            }
                            first = false;
                            out.extend(Self::name_of(&self.lists, base + bit));
                        }
                        3
                    }
                    0x8f => {
                        out.extend(Self::msg_of(&self.abilities, work(s.get(i + 2).copied().unwrap_or(0)) as u16 as i32 + 0x200));
                        3
                    }
                    // 7f 94 / 98 / 99 / 9a n: num[n] as %02d / %03d / %04d / %05d; 95 "$%x" of num[n]; 96 "%" and its binary
                    // digits; 97 "$%x" of work[n]; 9b work[n] x 0.1 as %.1f (FUN_100c1650; num read as work here).
                    0x94..=0x9b => {
                        let v = work(s.get(i + 2).copied().unwrap_or(0));
                        let t = match a {
                            0x94 => format!("{v:02}"),
                            0x98 => format!("{v:03}"),
                            0x99 => format!("{v:04}"),
                            0x9a => format!("{v:05}"),
                            0x95 | 0x97 => format!("${:x}", v),
                            0x96 => format!("%{:b}", v),
                            _ => format!("{:.1}", v as f32 * 0.1),
                        };
                        out.extend(t.bytes());
                        3
                    }
                    // Alternatives [a/b] (FUN_100bbc10); 99 picks none.
                    0x85..=0x88 | 0x90..=0x92 => {
                        let n = s.get(i + 2).copied().unwrap_or(0);
                        let k = match a {
                            0x86 => (work(n) != 1) as usize,
                            0x87 => who(n).is_some_and(|w| w.plural) as usize,
                            // No operand: 90 the caster, 91 the target.
                            0x90 => who(0).is_some_and(|w| w.female) as usize,
                            0x91 => who(1).is_some_and(|w| w.female) as usize,
                            0x88 => match who(n) {
                                Some(w) if w.pc => 99,
                                Some(w) => w.named as usize,
                                None => 0,
                            },
                            _ => 0,
                        };
                        let (text, len) = alternative(s, i, k);
                        out.extend(text);
                        len
                    }
                    // The handlers 0x80..0xb7 read an operand (3 bytes) but 0x8f..0x93; past the table (0x100c2b38) markers such as fb / fc are 2.
                    0x93 => 2,
                    0x80..=0xb7 => 3,
                    _ => 2,
                },
                0x20..0x7f => {
                    out.push(c);
                    1
                }
                _ => 1,
            };
            i += step.max(1);
        }
        while lines.len() > 1 && lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        Line { lines: lines.iter().map(|l| decode_sjis(l)).collect(), mode }
    }
}

/// FUN_100c2bf0's modes on ASCII text: 1 the first letter upper case, 2 all upper, 3 all lower, 4 each word's first.
fn apply_case(t: &mut [u8], mode: u8) {
    let mut word_start = true;
    let mut first = true;
    let mut i = 0;
    while i < t.len() {
        let c = t[i];
        if c >= 0x80 {
            i += 2;
            (first, word_start) = (false, false);
            continue;
        }
        match mode {
            1 if first && c.is_ascii_alphabetic() => t[i] = c.to_ascii_uppercase(),
            2 => t[i] = c.to_ascii_uppercase(),
            3 => t[i] = c.to_ascii_lowercase(),
            4 if word_start => t[i] = c.to_ascii_uppercase(),
            _ => {}
        }
        if c.is_ascii_alphabetic() {
            first = false;
        }
        word_start = c == b' ';
        i += 1;
    }
}

/// 1,234,567 (FUN_101c9440 in English).
pub fn separated(v: u32) -> String {
    let s = v.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A grammar argument (FUN_100bbd20): 0x40 | k = slot k; low bits 2 = 14-bit index over the next 3 bytes.
/// Bytes a grammar index takes: 1 (0x40 form) or 4 (form 2).
fn grammar_len(b: &[u8]) -> usize {
    match b.first() {
        Some(x) if x & 0x40 != 0 => 1,
        _ => 4,
    }
}

fn grammar_index(b: &[u8]) -> u16 {
    match b {
        [x, ..] if x & 0x40 != 0 => (x & 0x3f) as u16,
        [x, b1, b2, b3, ..] if x & 0x3f == 2 => (((b2 & 0x7f) as i32 | ((*b3 as i8 as i32) << 7)) << 7) as u16 | (b1 & 0x7f) as u16,
        _ => 0xffff,
    }
}

/// Alternative `k` of the `[a/b]` group after the code at `from`, and the bytes the code spans.
fn alternative(s: &[u8], from: usize, k: usize) -> (Vec<u8>, usize) {
    let Some(open) = s[from..].iter().position(|&c| c == b'[').map(|p| from + p) else { return (Vec::new(), 3) };
    let mut end = open + 1;
    while end < s.len() && s[end] != b']' && s[end] != 0 {
        end += if s[end] >= 0x80 { 2 } else { 1 };
    }
    let inner = &s[open + 1..end.min(s.len())];
    (inner.split(|&c| c == b'/').nth(k).unwrap_or_default().to_vec(), (end + 1).min(s.len()) - from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load() -> Option<BattleText> {
        let cfg = crate::config::Config::load();
        let index = DatIndex::open(&cfg.ffxi_dir()).ok()?;
        let dll = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")).ok()?;
        Some(BattleText::load(&index, &dll).unwrap())
    }

    /// The on-screen kinds (0x1035cae0): 1 hit damage, 15 miss, 24 recovers HP, 6 "defeats" none; Screen mode keeps
    /// a kinded line off the log, Log mode its number off the screen.
    #[test]
    fn screen_kinds() {
        let Some(b) = load() else { return };
        assert_eq!([1, 15, 24, 6, 185, 288].map(|id| b.screen_kind(id)), [1, 3, 2, 0, 1, 1]);
        let mut cfg = crate::gamecfg::Values::default();
        assert!(!b.off_channel(1, 0, &cfg) && !b.off_channel(1, 1, &cfg));
        cfg.set(0x13, 2);
        assert!(b.off_channel(1, 0, &cfg) && !b.off_channel(6, 0, &cfg));
        cfg.set(0x13, 0);
        assert!(b.off_channel(1, 1, &cfg) && b.off_channel(6, 1, &cfg));
    }

    /// 711, a trust's party line (LSB trustPartyMessage): "(name) " then grammar kind 0x88, table 1 (language slot
    /// 0x22) by work[0]: 11201 is Shantotto II's summon line.
    #[test]
    fn trust_party_line() {
        let Some(b) = load() else { return };
        let who = Who { name: "Valaineral".into(), named: true, ..Default::default() };
        let ctx = Ctx { actor: Some(who.clone()), target: Some(who), work: [11201, 0, 0, 0, 0], ..Default::default() };
        let l = b.expand(711, &ctx).unwrap();
        assert!(l.lines[0].starts_with("(Valaineral) The time is at hand"), "{:?}", l.lines);
    }

    /// 704 "You have undertaken \"%s\"." names the objective in work[0] (grammar kind 0x41, id + 0xe000).
    #[test]
    fn roe_objective_name() {
        let Some(b) = load() else { return };
        let ctx = Ctx { work: [1, 0, 0, 0, 0], ..Default::default() };
        let l = b.expand(704, &ctx).unwrap();
        assert!(l.lines[0].contains("First Step Forward"), "{:?}", l.lines);
    }

    /// DAT_1036eba0: <call> and <call0> are 0017, <call21> 0038; the <ncall> rows are left out.
    #[test]
    fn call_tags() {
        let Some(b) = load() else { return };
        assert_eq!(b.calls.first(), Some(&("<call>".to_string(), 17)));
        assert_eq!(b.calls.get(1), Some(&("<call0>".to_string(), 17)));
        assert_eq!(b.calls.last(), Some(&("<call21>".to_string(), 38)));
    }

    /// Grammar kinds 0x13 / 0x14: the status noun (206 "... stun effect wears off.") and adjective (204 "is no longer
    /// stunned.").
    #[test]
    fn status_noun_and_adjective() {
        let Some(b) = load() else { return };
        let ctx = Ctx { target: pc("Lnxtest"), work: [10, 0, 0, 0, 0], ..Default::default() };
        assert_eq!(b.expand(204, &ctx).unwrap().lines, ["Lnxtest is no longer stunned."]);
        assert_eq!(b.expand(206, &ctx).unwrap().lines, ["Lnxtest's stun effect wears off."]);
    }

    /// Message 180 (s2c 0x009): "You throw away a potion." for one, "... 3 potions." for more.
    #[test]
    fn throw_away_counts() {
        let Some(bt) = load() else { return };
        let ctx = |n: i32| Ctx { work: [4112, n, 0, 0, 0], ..Default::default() };
        assert_eq!(bt.system(180, &ctx(1)).unwrap().lines, vec!["You throw away a potion.".to_string()]);
        assert_eq!(bt.system(180, &ctx(3)).unwrap().lines, vec!["You throw away 3 potions.".to_string()]);
    }

    /// Delivery lines (7031 0xbe / 0xaa): 7f b4 n is "N gil" with separators, 1c n a string slot.
    #[test]
    fn delivery_gil_and_string() {
        let Some(bt) = load() else { return };
        let ctx = Ctx { work: [2, 0, 12345, 0, 0], strings: ["delivery".into(), String::new()], ..Default::default() };
        assert_eq!(bt.system(0xbe, &ctx).unwrap().lines, vec!["You take the 12,345 gil out of delivery slot 2.".to_string()]);
        let ctx = Ctx { work: [3, 1, 0, 0, 0], strings: ["Lnxcsfix".into(), String::new()], ..Default::default() };
        assert_eq!(bt.system(0xaa, &ctx).unwrap().lines, vec!["Slot 3:".to_string(), "1 gil, sent to you by Lnxcsfix.".to_string()]);
    }

    /// A skill up (message 38): 7f 9b prints work[1] x 0.1 as "%.1f"; /lastsynth check's material: 0a, then the
    /// singular with no article (0x111).
    #[test]
    fn skill_up_and_counts() {
        let Some(b) = load() else { return };
        let ctx = Ctx { target: pc("Lnxcsfix"), work: [49, 1, 0, 0, 0], ..Default::default() };
        assert_eq!(b.expand(38, &ctx).unwrap().lines, ["Lnxcsfix's woodworking skill rises 0.1 points."]);
        let ctx = Ctx { work: [698, 1, 0, 0, 0], ..Default::default() };
        assert_eq!(b.system(0x111, &ctx).unwrap().lines, ["1 ash log."]);
        let ctx = Ctx { work: [698, 2, 0, 0, 0], ..Default::default() };
        assert_eq!(b.system(0x111, &ctx).unwrap().lines, ["2 ash logs."]);
        let ctx = Ctx { work: [715, 1, 0, 0, 0], ..Default::default() };
        assert_eq!(b.system(0xa0, &ctx).unwrap().lines, ["You synthesized a piece of ash lumber."]);
    }

    /// Synthesis lines (7031 0xa4 / 0xcc): 7f 80 1 capitalises the article; grammar kind 1 is the article token.
    #[test]
    fn synthesis_lost_lines() {
        let Some(bt) = load() else { return };
        let ctx = Ctx { work: [698, 0, 0, 0, 0], ..Default::default() };
        assert_eq!(bt.system(0xa4, &ctx).unwrap().lines, vec!["An ash log was lost.".to_string()]);
        let ctx = Ctx { work: [698, 0, 0, 0, 0], strings: ["Lnxcsfix".into(), String::new()], ..Default::default() };
        assert_eq!(bt.system(0xcc, &ctx).unwrap().lines, vec!["Lnxcsfix lost an ash log.".to_string()]);
    }

    fn mob(name: &str) -> Option<Who> {
        Some(Who { name: name.into(), ..Default::default() })
    }

    /// s2c 0x009 /random (7031 88): string slots 2 / 3 = the roller and the roll.
    #[test]
    fn random_roll() {
        let Some(bt) = load() else { return };
        let ctx = Ctx { strings: ["Lnxtest".into(), "523".into()], ..Default::default() };
        assert_eq!(bt.system(88, &ctx).unwrap().lines, ["Dice roll! Lnxtest rolls 523!"]);
    }

    /// Treasure pool lines (s2c 0x0D2 / 0x0D3): the find (0x10, target the dropper), a lot (0x11: slot 2 the points,
    /// slot 3 the lotter) and the win (0x12: slot 2 the winner).
    #[test]
    fn treasure_lines() {
        let Some(bt) = load() else { return };
        let ctx = Ctx { target: mob("Wild Rabbit"), work: [698, 0, 0, 0, 0], ..Default::default() };
        assert_eq!(bt.system(0x10, &ctx).unwrap().lines, ["You find an ash log on the Wild Rabbit."]);
        let ctx = Ctx { target: mob("Wild Rabbit"), work: [698, 0, 0, 0, 0], strings: ["523".into(), "Lnxtest".into()], ..Default::default() };
        assert_eq!(bt.system(0x11, &ctx).unwrap().lines, ["Lnxtest's lot for the ash log: 523 points."]);
        let ctx = Ctx { target: mob("Wild Rabbit"), work: [698, 0, 0, 0, 0], strings: ["Lnxtest".into(), String::new()], ..Default::default() };
        assert_eq!(bt.system(0x12, &ctx).unwrap().lines, ["Lnxtest obtains an ash log."]);
    }

    /// File 7025: /wave is entries 16 (a target) and 17; /no takes the caster's [his/her].
    #[test]
    fn emote_lines() {
        let Some(bt) = load() else { return };
        let me = |female| Some(Who { name: "Lnxtest".into(), pc: true, female, ..Default::default() });
        let ctx = Ctx { actor: me(false), target: Some(Who { name: "Rahal".into(), named: true, ..Default::default() }), ..Default::default() };
        assert_eq!(bt.emote(17, &ctx).unwrap().lines, ["Lnxtest waves."]);
        assert_eq!(bt.emote(16, &ctx).unwrap().lines, ["Lnxtest waves to Rahal."]);
        let line = |female| bt.emote(13, &Ctx { actor: me(female), ..Default::default() }).unwrap().lines.join(" ");
        assert!(line(false).contains("his") && line(true).contains("her"));
    }

    fn pc(name: &str) -> Option<Who> {
        Some(Who { name: name.into(), pc: true, ..Default::default() })
    }

    #[test]
    fn basic_messages() {
        let Some(b) = load() else { return };
        let ctx = Ctx { actor: pc("Tuxedo"), target: mob("Wild Rabbit"), work: [0, 5, 0, 0, 0], ..Default::default() };
        assert_eq!(b.expand(1, &ctx).unwrap().lines, ["Tuxedo hits the Wild Rabbit for 5 points of damage."]);
        let one = Ctx { work: [0, 1, 0, 0, 0], ..ctx.clone() };
        assert_eq!(b.expand(1, &one).unwrap().lines, ["Tuxedo hits the Wild Rabbit for 1 point of damage."]);
        let back = Ctx { actor: mob("Wild Rabbit"), target: pc("Tuxedo"), ..ctx.clone() };
        assert_eq!(b.expand(1, &back).unwrap().lines, ["The Wild Rabbit hits Tuxedo for 5 points of damage."]);
        assert_eq!(b.expand(15, &back).unwrap().lines, ["The Wild Rabbit misses Tuxedo."]);
        assert_eq!(b.expand(6, &ctx).unwrap().lines, ["Tuxedo defeats the Wild Rabbit."]);
        // Named (proper noun) and plural monsters: "Ding Bats hit".
        let bats = Ctx { actor: Some(Who { name: "Ding Bats".into(), named: true, plural: true, pc: false, female: false }), target: pc("Tuxedo"), ..ctx.clone() };
        assert_eq!(b.expand(1, &bats).unwrap().lines, ["Ding Bats hit Tuxedo for 5 points of damage."]);
        // Experience (0x02D work[0]) and level up, which sets mode 0x79 with 1f 'y'.
        let exp = Ctx { actor: pc("Tuxedo"), target: pc("Tuxedo"), work: [50, 0, 0, 0, 0], ..Default::default() };
        assert_eq!(b.expand(8, &exp).unwrap().lines, ["Tuxedo gains 50 experience points."]);
        let lv = Ctx { work: [2, 0, 0, 0, 0], ..exp };
        let l = b.expand(9, &lv).unwrap();
        assert_eq!((l.lines.as_slice(), l.mode), (["Tuxedo attains level 2!".to_string()].as_slice(), Some(0x79)));
        // Spell names (55702) and two-line messages.
        let cure = Ctx { actor: pc("Tuxedo"), target: pc("Tuxedo"), work: [1, 12, 0, 0, 0], ..Default::default() };
        assert_eq!(b.expand(7, &cure).unwrap().lines, ["Tuxedo casts Cure.", "Tuxedo recovers 12 HP."]);
    }

    #[test]
    fn modes_and_colours() {
        let Some(b) = load() else { return };
        // "hits": ours 0x14, on us 0x1c, others 0x28 (FFXiMain tables 0x1034cf60 / d360 / e760).
        assert_eq!([Relation::CasterIsMe, Relation::TargetIsMe, Relation::Other].map(|r| b.mode(1, r)), [0x14, 0x1c, 0x28]);
        assert_eq!(b.mode(13, Relation::Other), HIDDEN);
        // Damage taken is red, system lines yellow.
        assert_eq!(b.colour(0x1c), [0x2c, 0x30, 0xa0, 0x70]);
        assert_eq!(b.colour(0x7a), [0x10, 0x80, 0x80, 0x60]);
        assert_eq!(b.colour(0xff), [0x80; 4]);
    }

    /// The install predates LSB's client: ids match up to the Master Level messages, which are blank here.
    #[test]
    fn ids_match_lsb() {
        let Some(b) = load() else { return };
        let ctx = Ctx { actor: pc("A"), target: mob("B"), work: [3, 4, 5, 6, 0], me: "A".into(), ..Default::default() };
        let t = |id| b.expand(id, &ctx).map(|l| l.lines.join(" ")).unwrap_or_default();
        assert_eq!(t(264), "The B takes 4 points of damage.");
        assert_eq!(t(282), "The B evades.");
        assert_eq!(t(718), "A gains 3 capacity points.");
        assert_eq!(t(777), "You cannot mount at this time.");
        assert_eq!(t(809), "A gains 3 exemplar points.");
    }
}
