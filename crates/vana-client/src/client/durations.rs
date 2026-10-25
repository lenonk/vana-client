//! How long the effects we put on others last, as LSB reckons them (not retail: no client knows; spec
//! ~/ffxi/lab/research/timers_durations_spec.md, tables data/timers/*.tsv made by ~/ffxi/lab/tools/timers_data.py):
//! which 0x028 result lands which effect, its length from our job, levels, merits, job points, gear and effects,
//! and what it replaces. Resist rolls and random terms are unknown: those lengths are upper bounds, cut short by the
//! server's wear-off message.

use super::World;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// The table an action is looked up in (spec 1.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Spell,
    Ability,
    Ws,
    /// Category 13 (avatar, wyvern: the ability id) and 11 (jug pet, luopan, automaton: the mob skill id).
    Pet,
}

/// A modifier token of a row (the inputs its formula reads).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tok {
    Mod(u16),
    Merit(u16),
    Jp(u16),
    Status(u16),
    TStatus(u16),
    TMod(u16),
    Lvl(u8),
    Skill(u8),
    Aoe(u16),
    Resist,
    Nm,
    Random,
    States(u8),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Base {
    Secs(f32),
    /// A random length: the longest is predicted.
    Range(f32, f32),
    None,
}

/// One (action, effect) of the table.
#[derive(Clone, Debug)]
pub struct Row {
    pub id: u16,
    pub target: String,
    pub effect: u16,
    /// What the result's value holds: E the effect, D damage, R a roll total, S a step level, G a geomancy effect,
    /// V Valiance's 531, H HP healed, A the target's Absorb down, 0 nothing to check.
    pub param: char,
    pub landed: Vec<u16>,
    pub base: Base,
    pub formula: String,
    pub toks: Vec<Tok>,
    /// y every input known, p up to a resist or random term, n the landing is never shown, s self only.
    pub know: char,
    pub tier: u8,
}

impl Row {
    fn has(&self, t: Tok) -> bool {
        self.toks.contains(&t)
    }

    fn base(&self) -> f32 {
        match self.base {
            Base::Secs(s) => s,
            Base::Range(_, hi) => hi,
            Base::None => 0.0,
        }
    }
}

/// A status effect's container rules (status_effects.tsv).
#[derive(Clone, Debug, Default)]
pub struct Rule {
    pub flags: String,
    pub overwrite: String,
    pub exclusion: Option<String>,
    pub negative: Option<u16>,
    pub remove: Option<u16>,
    pub min: f32,
}

pub struct Tables {
    pub rows: HashMap<(Kind, u16), Vec<Row>>,
    pub rules: HashMap<u16, Rule>,
    /// Item -> (mod, value, item level, latent: 0 none, 1 in Dynamis, 2 in Assault).
    gear: HashMap<u16, Vec<(u16, i32, u8, u8)>>,
    /// Augment -> (multiplier, mod, value).
    augments: HashMap<u16, Vec<(i32, u16, i32)>>,
    /// Merit -> (value per upgrade, job bits (0 general), needs 96).
    merits: HashMap<u16, (f32, u32, bool)>,
    /// Job point -> its job.
    jps: HashMap<u16, u8>,
}

const ACTIONS: &str = include_str!("../../data/timers/actions.tsv");
const STATUS: &str = include_str!("../../data/timers/status.tsv");
const GEAR: &str = include_str!("../../data/timers/gear.tsv");
const AUGMENTS: &str = include_str!("../../data/timers/augments.tsv");
const MERITS: &str = include_str!("../../data/timers/merits.tsv");
const JOBPOINTS: &str = include_str!("../../data/timers/jobpoints.tsv");

fn lines(t: &str) -> impl Iterator<Item = Vec<&str>> {
    t.lines().filter(|l| !l.starts_with('#') && !l.is_empty()).map(|l| l.split('\t').collect())
}

fn num<T: std::str::FromStr>(s: &str) -> Option<T> {
    s.trim().parse().ok()
}

pub fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let mut rows: HashMap<(Kind, u16), Vec<Row>> = HashMap::new();
        for f in lines(ACTIONS).filter(|f| f.len() >= 11) {
            let kind = match f[0] {
                "spell" => Kind::Spell,
                "ability" => Kind::Ability,
                "ws" => Kind::Ws,
                "pet" => Kind::Pet,
                _ => continue,
            };
            let Some(id) = num(f[1]) else { continue };
            let base = match f[6].split_once('-') {
                _ if f[6] == "-" => Base::None,
                Some((a, b)) => Base::Range(num(a).unwrap_or(0.0), num(b).unwrap_or(0.0)),
                None => num(f[6]).map_or(Base::None, Base::Secs),
            };
            let toks = f[8]
                .split(',')
                .filter_map(|t| {
                    let (k, v) = t.split_once(':').unwrap_or((t, ""));
                    let n = || num::<u16>(v);
                    Some(match k {
                        "mod" => Tok::Mod(n()?),
                        "merit" => Tok::Merit(n()?),
                        "jp" => Tok::Jp(n()?),
                        "status" => Tok::Status(n()?),
                        "tstatus" => Tok::TStatus(n()?),
                        "tmod" => Tok::TMod(n()?),
                        "lvl" => Tok::Lvl(n()? as u8),
                        "skill" => Tok::Skill(n()? as u8),
                        "aoe" => Tok::Aoe(n()?),
                        "states" => Tok::States(n()? as u8),
                        "resist" => Tok::Resist,
                        "nm" => Tok::Nm,
                        "random" => Tok::Random,
                        _ => return None,
                    })
                })
                .collect();
            rows.entry((kind, id)).or_default().push(Row {
                id,
                target: f[2].to_string(),
                effect: num(f[3]).unwrap_or(0),
                param: f[4].chars().next().unwrap_or('0'),
                landed: f[5].split(',').filter_map(num).collect(),
                base,
                formula: f[7].to_string(),
                toks,
                know: f[9].chars().next().unwrap_or('-'),
                tier: num(f[10]).unwrap_or(0),
            });
        }
        let opt = |s: &str| (s != "-").then(|| s.to_string());
        let rules = lines(STATUS)
            .filter(|f| f.len() >= 9)
            .filter_map(|f| {
                Some((
                    num(f[0])?,
                    Rule { flags: f[1].to_string(), overwrite: f[2].to_string(), exclusion: opt(f[3]), negative: num(f[4]), remove: num(f[6]), min: num(f[7]).unwrap_or(0.0) },
                ))
            })
            .collect();
        let mut gear: HashMap<u16, Vec<(u16, i32, u8, u8)>> = HashMap::new();
        for f in lines(GEAR).filter(|f| f.len() >= 5) {
            let latent = match f[4] {
                "dynamis" => 1,
                "assault" => 2,
                _ => 0,
            };
            if let (Some(i), Some(m), Some(v)) = (num(f[0]), num(f[1]), num(f[2])) {
                gear.entry(i).or_default().push((m, v, num(f[3]).unwrap_or(0), latent));
            }
        }
        let mut augments: HashMap<u16, Vec<(i32, u16, i32)>> = HashMap::new();
        for f in lines(AUGMENTS).filter(|f| f.len() >= 4) {
            if let (Some(a), Some(k), Some(m), Some(v)) = (num(f[0]), num(f[1]), num(f[2]), num(f[3])) {
                augments.entry(a).or_default().push((k, m, v));
            }
        }
        let merits = lines(MERITS).filter(|f| f.len() >= 4).filter_map(|f| Some((num(f[0])?, (num(f[1])?, num(f[2])?, f[3] == "1")))).collect();
        let jps = lines(JOBPOINTS).filter(|f| f.len() >= 2).filter_map(|f| Some((num(f[0])?, num(f[1])?))).collect();
        Tables { rows, rules, gear, augments, merits, jps }
    })
}

/// Our side of a prediction: jobs, merits, job points, gear and effect mods, skills, TP.
#[derive(Clone, Debug, Default)]
pub struct Caster {
    pub main: (u8, u8),
    pub statuses: HashSet<u16>,
    pub mods: HashMap<u16, f32>,
    pub merits: HashMap<u16, u8>,
    pub jps: HashMap<u16, u8>,
    /// Working skills by skill id (s2c 0x062).
    pub skills: HashMap<u8, f32>,
    /// TP at the weapon skill's start, our pet's TP.
    pub tp: f32,
    pub pet_tp: Option<f32>,
    /// The ranged slot holds a string or wind instrument; the ammo item.
    pub instrument: bool,
    pub ammo: u16,
    /// A pet casting: none of our inputs apply.
    pub pet: bool,
}

/// Dynamis and Assault zones for the two latent rows (Millennium Horn, Storm Fife; the Assault list inferred).
const DYNAMIS: [u16; 14] = [39, 40, 41, 42, 134, 135, 185, 186, 187, 188, 294, 295, 296, 297];
const ASSAULT: [u16; 7] = [55, 56, 63, 64, 66, 69, 77];

/// Merit level caps by main level (data/merits.yaml).
fn merit_cap(level: u8) -> u8 {
    match level {
        80.. => 15,
        75.. => 10,
        70.. => 9,
        65.. => 8,
        60.. => 7,
        55.. => 6,
        50.. => 5,
        40.. => 4,
        30.. => 3,
        20.. => 2,
        10.. => 1,
        _ => 0,
    }
}

impl Caster {
    /// Our snapshot; `lost` are effects that left us within the last moment (the cast that used them reports
    /// after their wear-off).
    pub fn of(w: &World, lost: &[u16], tp: Option<f32>) -> Self {
        let main = w.stats.as_ref().map_or((0, 0), |s| s.main);
        let mut statuses: HashSet<u16> = w.buffs.iter().copied().filter(|&b| b != 0xffff && b != 0).collect();
        statuses.extend(lost.iter().copied());
        let mut c = Caster {
            main,
            statuses,
            merits: w.merit_list.iter().map(|(&id, &(_, n))| (id, n)).collect(),
            jps: w.jp_list.iter().map(|(&id, &(lv, _))| (id, lv)).collect(),
            skills: w.skill_base.map_or(HashMap::new(), |s| (0..64).map(|i| (i as u8, (s[i] & 0x7fff) as f32)).collect()),
            tp: tp.unwrap_or(1000.0),
            pet_tp: w.timers.pet_tp.map(|t| t as f32),
            ..Default::default()
        };
        // Equipped gear: its mods and augments.
        let info = w.item_info.clone();
        for slot in 0..16 {
            let Some(item) = slot_item(w, slot) else { continue };
            if slot == 2 {
                c.instrument = info.as_ref().and_then(|i| i.item(item)).is_some_and(|it| matches!(it.skill, 41 | 42));
            }
            if slot == 3 {
                c.ammo = item;
            }
            for (m, v) in item_mods(w, slot) {
                *c.mods.entry(m).or_default() += v;
            }
        }
        c.effect_mods(w);
        c
    }

    /// Mods our own effects add (spec 90.1 item 3) and the BRD job point gift.
    fn effect_mods(&mut self, w: &World) {
        let add = |c: &mut Caster, m: u16, v: f32| *c.mods.entry(m).or_default() += v;
        // REGEN_DURATION from Light Arts / Addendum: White and Tabula Rasa (SCH main).
        let s = if self.main.0 == 20 && self.main.1 >= 20 { 3.0 * ((self.main.1 as f32 - 10.0) / 10.0).floor() } else { 0.0 };
        let arts_w = self.has(358) || self.has(401);
        if arts_w {
            add(self, 339, 2.0 * s);
        }
        if self.has(377) {
            let s2 = (1.5 * s).floor();
            add(self, 339, if arts_w { (2.0 * s2 / 1.5).ceil() } else { 2.0 * s2 });
            add(self, 477, 108.0);
        } else if self.has(359) || self.has(402) {
            add(self, 477, 72.0);
        }
        // DARK_MAGIC_DURATION from Dark Seal (merit x ENHANCES_DARK_SEAL / 10).
        if self.has(345) {
            let v = self.merit(DARK_SEAL) * self.m(1073) / 10.0;
            add(self, 1072, v);
        }
        // SAMBA_PDURATION from Saber Dance merits.
        if self.has(410) {
            let v = self.merit(SABER_DANCE) - 5.0;
            if v > 0.0 {
                add(self, 498, v);
            }
        }
        // ENH_MAGIC_DURATION from Naturalist's Roll on us: its power by the total we saw land.
        if self.has(339)
            && let Some(total) = w.timers.roll_on_me(339)
        {
            const POWER: [f32; 12] = [6.0, 7.0, 15.0, 8.0, 9.0, 10.0, 5.0, 11.0, 12.0, 13.0, 20.0, -5.0];
            add(self, 890, POWER[(total.clamp(1, 12) - 1) as usize]);
        }
        // Clarion Call: one more song a target.
        if self.has(499) {
            add(self, 453, 1.0);
        }
        // BRD 1200 job points spent, main BRD 99: SONG_DURATION_BONUS +5.
        if self.main == (10, 99) && w.jp_jobs.is_some_and(|j| j[10][2] >= 1200) {
            add(self, 454, 5.0);
        }
    }

    pub fn has(&self, s: u16) -> bool {
        !self.pet && self.statuses.contains(&s)
    }

    pub fn m(&self, id: u16) -> f32 {
        if self.pet {
            return 0.0;
        }
        self.mods.get(&id).copied().unwrap_or(0.0)
    }

    /// getMerit: the upgrades (at most the level's cap) times the value, for a general merit or one of our main
    /// job at main level 75 (96 for weapon skill merits).
    pub fn merit(&self, id: u16) -> f32 {
        if self.pet {
            return 0.0;
        }
        let Some(&(v, jobs, ws)) = tables().merits.get(&id) else { return 0.0 };
        if jobs != 0 && (jobs & (1 << self.main.0) == 0 || self.main.1 < if ws { 96 } else { 75 }) {
            return 0.0;
        }
        self.merits.get(&id).copied().unwrap_or(0).min(merit_cap(self.main.1)) as f32 * v
    }

    /// getJobPointLevel: only for our main job at 99.
    pub fn jp(&self, id: u16) -> f32 {
        if self.pet || self.main.1 < 99 || tables().jps.get(&id) != Some(&self.main.0) {
            return 0.0;
        }
        self.jps.get(&id).copied().unwrap_or(0) as f32
    }

    fn skill(&self, id: u8) -> f32 {
        self.skills.get(&id).copied().unwrap_or(0.0)
    }
}

/// The item in an equipment slot.
pub fn slot_item(w: &World, slot: usize) -> Option<u16> {
    let at = w.inv.equip.get(slot)?.as_ref()?;
    w.inv.slots.get(at).map(|s| s.0)
}

/// An equipped item's duration mods: its table rows (latents by zone) and its augments, none while our main level is
/// below the item's (level sync, spec 90.1).
pub fn item_mods(w: &World, slot: usize) -> Vec<(u16, f32)> {
    let t = tables();
    let Some(item) = slot_item(w, slot) else { return Vec::new() };
    let main = w.stats.as_ref().map_or(0, |s| s.main.1);
    let mut out = Vec::new();
    for &(m, v, level, latent) in t.gear.get(&item).map_or(&[][..], |g| &g[..]) {
        let on = match latent {
            1 => DYNAMIS.contains(&w.pos_zone),
            2 => ASSAULT.contains(&w.pos_zone),
            _ => true,
        };
        if on && main >= level {
            out.push((m, v as f32));
        }
    }
    // LSB reads slots 0..3 of every equipment's extdata as standard augments (spec 90.5).
    let level = w.item_info.as_ref().and_then(|i| i.item(item)).map_or(0, |it| it.level);
    if let Some(ext) = w.inv.equip[slot].and_then(|at| w.inv.ext.get(&at))
        && main as u16 >= level
    {
        for k in 0..4 {
            let raw = u16::from_le_bytes([ext[2 + 2 * k], ext[3 + 2 * k]]);
            let (id, v) = (raw & 0x7ff, (raw >> 11) as i32);
            for &(mult, m, base) in t.augments.get(&id).map_or(&[][..], |a| &a[..]) {
                out.push((m, ((if base > 0 { base + v } else { base - v }) * mult.max(1)) as f32));
            }
        }
    }
    out
}

/// One slot's value of a mod.
pub fn slot_mod(w: &World, slot: usize, m: u16) -> f32 {
    item_mods(w, slot).into_iter().filter(|x| x.0 == m).map(|x| x.1).sum()
}

const DARK_SEAL: u16 = 2496;
const SABER_DANCE: u16 = 3200;

/// The target's side we know.
#[derive(Clone, Debug, Default)]
pub struct Target {
    pub me: bool,
    /// A party member's main level (0x0DD), for the enhancing level penalty.
    pub level: Option<u8>,
    /// Effects we can see on it (our own icons, or a party member's 0x076 list).
    pub has: HashSet<u16>,
    /// The first target of the action (Saboteur covers only it).
    pub first: bool,
    /// The target's mods when it is us (Embolden, en-spell, sneak / invisible gear).
    pub mods: HashMap<u16, f32>,
}

/// Spells of the white magic group (Perpetuance, Embolden): the spell record's kind 1.
pub fn white(w: &World, spell: u16) -> bool {
    w.actions.as_ref().and_then(|a| a.spell(spell)).is_some_and(|s| s.kind == 1)
}

/// The weapon skill TP formula (spec 70.9) for the TP-scaled rows.
fn ws_tp(id: u16, tp: f32) -> Option<f32> {
    let f = |a: f32, b: f32, c: f32| if tp <= 2000.0 { a + (b - a) * (tp - 1000.0).max(0.0) / 1000.0 } else { b + (c - b) * (tp - 2000.0) / 1000.0 };
    Some(match id {
        83 | 181 | 80 => 120.0 + 6.0 * tp / 100.0,
        85 => 120.0 + 6.0 * tp / 100.0,
        87 => 60.0 + 3.0 * tp / 100.0,
        138 | 187 | 155 | 58 | 219 | 99 | 188 | 29 | 186 | 52 => 6.0 * tp / 100.0,
        107 => 18.0 * tp / 100.0,
        102 => 30.0 + 3.0 * tp / 100.0,
        129 => f(30.0, 60.0, 120.0),
        139 | 16 => 75.0 + 15.0 * tp / 1000.0,
        17 => 30.0 + 6.0 * tp / 100.0,
        224 => 45.0 + 45.0 * tp / 1000.0,
        162 | 2 | 65 | 239 => tp / 500.0,
        15 => 15.0 + 3.0 * tp / 1000.0,
        _ => return None,
    })
}

/// The jig's base (Spectral Jig 180, Chocobo Jig 120).
fn jig_base(id: u16) -> f32 {
    if id == 196 { 180.0 } else { 120.0 }
}

/// Repair's regen by the oil in the ammo slot.
fn repair_secs(ammo: u16) -> f32 {
    match ammo {
        18731 => 15.0,
        18732 => 30.0,
        18733 => 45.0,
        19185 => 60.0,
        _ => 0.0,
    }
}

/// LSB's length for one row landing on `t`; None when there is nothing to time (self-only rows the server times,
/// silent or hidden effects, removals). `prior`: the time left on our timer of the same effect there (steps).
pub fn predict(c: &Caster, row: &Row, t: &Target, white: bool, prior: Option<f32>) -> Option<f32> {
    let base = row.base();
    let d = match row.formula.as_str() {
        "FIXED" | "NIN_FIXED" | "BLU_ENF" | "BLU_ENF_RAW" | "DIA_BIO" | "MOB_STATUS" | "MOB_STATUS_EX" | "MOB_GAZE" | "RANDOM" | "WS_R" | "WS_FIXED" | "QD_SLEEP" | "DESPERATE" | "HOLY2_AMNESIA" | "JA_CHARM_BIND" | "SOLACE_SS" | "ARISE_RR" => base,
        "ENH" | "ENH_REGEN" | "ENH_RANDOM" | "ENH_EN" => enhancing(c, row, t, white),
        "ENF" => enfeebling(c, row, t),
        "HELIX" => {
            let mut d = 30.0;
            if c.main.1 >= 60 {
                d += 60.0;
            } else if c.main.1 >= 40 {
                d += 30.0;
            }
            if c.has(359) {
                d += 3.0 * c.jp(643);
            }
            (d + c.m(477)).floor()
        }
        "KAUSTRA" => (3.0 * (1.0 + c.skill(37).clamp(0.0, 500.0) / 11.0)).floor(),
        "IMPACT" => 180.0,
        "ABSORB_STAT" | "ABSORB_SELF" => {
            let b = (180.0 + ((c.skill(37) - 490.5) / 5.0).floor()).clamp(0.0, 10000.0);
            (b * (1.0 + c.m(1072) / 100.0) * (1.0 + c.m(1138) / 100.0)).floor() + c.m(1136)
        }
        "DRAIN_MAXHP" => 180.0 * (1.0 + c.m(1072) / 100.0),
        "ENH_SONG" | "ENH_SONG_SVD" => {
            let b = song_mod(c, row) + c.m(452);
            let mut d = (120.0 * (1.0 + 0.1 * b + c.m(454) / 100.0)).floor();
            if c.has(499) {
                d += 2.0 * c.jp(322);
            }
            if c.has(231) {
                d += c.jp(328);
            }
            if c.has(455) {
                d += 2.0 * c.jp(326);
            }
            if row.formula == "ENH_SONG_SVD" {
                if c.has(52) {
                    d *= 2.0;
                } else if c.has(231) {
                    d = (d * 1.5).floor();
                }
            }
            if c.has(348) {
                d *= 2.0;
            }
            d
        }
        "ENF_SONG" => {
            let gb = song_mod(c, row) + c.m(452);
            let mut d = (base * (1.0 + gb / 10.0 + c.m(454) / 100.0)).floor();
            if row.effect == 2 {
                d += c.jp(327);
            }
            if c.has(499) {
                d += 2.0 * c.jp(322);
            }
            if c.has(455) {
                d += 2.0 * c.jp(326);
            }
            if c.has(348) {
                d *= 2.0;
            }
            d
        }
        "BLU_BUFF" | "BLU_BUFF_DIFF2" => {
            // Diffusion's merit reaches only the caster (LSB, spec 40.3).
            let m = c.merit(3010);
            if c.has(356) && t.me && m > 0.0 {
                if row.formula == "BLU_BUFF" { base + (m - 5.0) * base / 100.0 } else { base + base / 100.0 * m }
            } else {
                base
            }
        }
        "INDI" => 180.0 + c.m(960),
        "BP_WARD" | "BP_WARD*R" => base + (c.skill(38) - 300.0).clamp(0.0, 200.0),
        "BP_PD" => 30.0 + c.skill(38).min(600.0) / 20.0,
        "BP_NIGHTMARE" => 90.0,
        "MOB_TP_SCALED" => {
            let tp = c.pet_tp.unwrap_or(1000.0);
            (180.0 + 180.0 * (tp - 1000.0) / 1000.0).clamp(180.0, 540.0)
        }
        "ROLL" => 300.0 + c.merit(3076) + c.m(882) + 2.0 * c.jp(545),
        "STEP" => {
            let jp = c.jp(609);
            match prior {
                Some(left) => (left.floor() + 30.0 + jp).min(120.0 + jp),
                None => 60.0 + jp,
            }
        }
        "JIG" => ((jig_base(row.id) + c.jp(613)) * (1.0 + c.m(492).clamp(0.0, 50.0) / 100.0)).floor(),
        "SAMBA" => (120.0 + c.m(490) + 2.0 * c.jp(611)) * (100.0 + c.m(498)) / 100.0,
        "RUNE" => 300.0,
        "VALLATION" => 120.0 + c.jp(707),
        "VALIANCE" => 180.0 + c.jp(707),
        "OFA" => 30.0 + c.jp(712),
        "LIEMENT" => 10.0 + c.m(1017),
        "GAMBIT" => 60.0 + c.jp(713) + c.m(1018),
        "RAYKE" => 27.0 + c.merit(3458) + c.m(1014) * c.merit(3458) / 3.0,
        "PLUS_MOD" => base + row.toks.iter().map(|t| if let Tok::Mod(m) = t { c.m(*m) } else { 0.0 }).sum::<f32>(),
        "PLUS_JP" => base + row.toks.iter().map(|t| if let Tok::Jp(j) = t { c.jp(*j) } else { 0.0 }).sum::<f32>(),
        "JA_KILLER_INSTINCT" => 180.0 + c.merit(2562) - 10.0,
        "JA_TOMAHAWK" => 30.0 + c.merit(2050) - 15.0,
        "JA_ANGON" => 15.0 + c.merit(2882),
        "JA_PENANCE" => c.merit(2118),
        "JA_SHADOWBIND" => 30.0 + c.m(425) + c.jp(357),
        "JA_FERAL_HOWL" => 10.0 + if c.m(503) >= 1.0 { c.merit(2560) / 5.0 } else { 0.0 },
        "JA_REPAIR" => repair_secs(c.ammo),
        "WS_TPR" | "WS_TP_PART_R" | "WS_TP_NOR" => ws_tp(row.id, c.tp.min(3000.0))?.floor(),
        _ => return None,
    };
    // The container raises it to the effect's least length (status_effect_container.cpp:503).
    let min = tables().rules.get(&row.effect).map_or(0.0, |r| r.min);
    (d > 0.0).then_some(d.max(min))
}

/// The song family's mod (the row's first song mod: 433..450 but the all-songs and duration ones).
fn song_mod(c: &Caster, row: &Row) -> f32 {
    row.toks.iter().find_map(|t| if let Tok::Mod(m @ 433..=450) = t { (!matches!(m, 444 | 446 | 449 | 452..=454)).then(|| c.m(*m)) } else { None }).unwrap_or(0.0)
}

/// ENH (spec 10.2).
fn enhancing(c: &Caster, row: &Row, t: &Target, white: bool) -> f32 {
    let mut d = match row.formula.as_str() {
        // Sneak / Invisible / Deodorize: 420 and a random 0 / 60 / 120 (the longest).
        "ENH_RANDOM" => 420.0 + 120.0,
        _ => row.base(),
    };
    let tmod = |m: u16| t.mods.get(&m).copied().unwrap_or(0.0);
    if white && t.has.contains(&534) && !c.pet {
        d *= 0.5 + tmod(1019) / 100.0;
    }
    d += d * c.m(890) / 100.0;
    if c.main.0 == 5 {
        d += c.merit(2320) + c.jp(169);
    }
    match row.formula.as_str() {
        "ENH_REGEN" => d += c.m(339) + 3.0 * c.jp(104),
        "ENH_RANDOM" if row.effect == 69 => d += tmod(947),
        "ENH_RANDOM" if row.effect == 71 => d += tmod(946),
        "ENH_EN" => d += tmod(938),
        _ => {}
    }
    if row.has(Tok::Status(419)) && c.has(419) && t.me {
        d *= 3.0;
    }
    if c.has(469) && white {
        d *= 2.0;
    }
    if let (Some(lv), Some(tl)) = (row.toks.iter().find_map(|t| if let Tok::Lvl(l) = t { Some(*l) } else { None }), t.level)
        && tl < lv
    {
        d *= tl as f32 / lv as f32;
    }
    d
}

/// ENF (spec 20.3), with the resist rate at 1 (an upper bound) and the monster taken as no NM.
fn enfeebling(c: &Caster, row: &Row, t: &Target) -> f32 {
    let mut d = match row.effect {
        11 => 60.0,
        // Stun floors its random 2..8.
        10 => 7.0,
        _ => row.base(),
    };
    if (128..=133).contains(&row.effect) {
        d += c.merit(2258);
    }
    // Enfeebling Magic skill: the rows that read Saboteur.
    if row.has(Tok::Status(454)) {
        if c.has(454) && t.first {
            d *= 2.0;
        }
        if c.main.0 == 5 {
            d += c.merit(2316) + c.jp(167);
            if c.has(494) {
                d += c.jp(162);
            }
        }
        d = (d * (1.0 + c.m(1151) / 100.0)).floor();
    }
    d = d.floor();
    if row.effect == 11 {
        d = d.clamp(5.0, 60.0);
    }
    d
}

/// Whether a result lands this row: its message, and the value the row's param kind expects.
pub fn lands(row: &Row, message: u16, value: u32) -> bool {
    if !row.landed.contains(&message) {
        return false;
    }
    match row.param {
        'E' => value == row.effect as u32,
        'D' if row.formula == "HELIX" => value > 0,
        'R' => (1..=11).contains(&value),
        'S' | 'G' => value > 0,
        'V' => value == 531,
        _ => true,
    }
}

/// The icons a status shows as (the 0x029 / 0x076 data): sleep II and lullaby as sleep, Copy Image by its
/// shadows, Doubt as Intimidate (spec 80.4.2).
pub fn icon_matches(status: u16, icon: u16) -> bool {
    status == icon
        || match status {
            19 | 193 => icon == 2,
            66 => matches!(icon, 444..=446),
            576 => icon == 22,
            _ => false,
        }
}

/// Effects drawn with icon 0 or the no-loss flag: the server never says they ended.
pub fn silent_end(status: u16) -> bool {
    matches!(status, 793 | 798 | 801 | 805 | 806) || tables().rules.get(&status).is_some_and(|r| r.flags.contains('n'))
}

/// The status rule's flag (d death, z zone, n no loss message ...).
pub fn flag(status: u16, f: char) -> bool {
    tables().rules.get(&status).is_some_and(|r| r.flags.contains(f))
}

/// Statuses a new one ends on the same target (spec 1.5): its `remove`, `negative` and exclusion group, Dia vs Bio,
/// the stat boosts' and storms' own exclusivity. The same status is the caller's (several kinds keep copies).
pub fn ends(status: u16) -> Vec<u16> {
    let t = tables();
    let Some(r) = t.rules.get(&status) else { return Vec::new() };
    let mut out: Vec<u16> = r.remove.into_iter().chain(r.negative).filter(|&s| s != status).collect();
    if let Some(g) = &r.exclusion {
        out.extend(t.rules.iter().filter(|(id, o)| **id != status && o.exclusion.as_ref() == Some(g)).map(|(id, _)| *id));
    }
    match status {
        134 => out.push(135),
        80..=86 => out.extend((80..=86).filter(|&s| s != status)),
        178..=185 => out.extend((178..=185).filter(|&s| s != status)),
        _ => {}
    }
    out
}

/// Several copies of these coexist on a target (songs, maneuvers, runes, bust): keyed by tier too.
pub fn duplicates(status: u16) -> bool {
    tables().rules.get(&status).is_some_and(|r| r.overwrite == "ignore_duplicate")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: Kind, id: u16) -> &'static Row {
        &tables().rows[&(kind, id)][0]
    }

    fn rdm99() -> Caster {
        Caster { main: (5, 99), ..Default::default() }
    }

    /// The tables load: every action family is there.
    #[test]
    fn tables_load() {
        let t = tables();
        assert!(t.rows.len() > 800, "{}", t.rows.len());
        assert_eq!(t.rules[&2].overwrite, "higher");
        assert!(t.gear.contains_key(&11148));
        assert_eq!(row(Kind::Spell, 57).formula, "ENH");
        assert_eq!(row(Kind::Spell, 57).effect, 33);
    }

    /// Spec 20.3's worked example: RDM 99, 5/5 enfeebling duration merits, 20 job points, ENF_MAG_DURATION 20,
    /// Saboteur on a normal mob, Slow: 180 x 2 + 30 + 20 = 410, x 1.2 = 492.
    #[test]
    fn slow_example() {
        let mut c = rdm99();
        c.merits.insert(2316, 5);
        c.jps.insert(167, 20);
        c.mods.insert(1151, 20.0);
        c.statuses.insert(454);
        let t = Target { first: true, ..Default::default() };
        assert_eq!(predict(&c, row(Kind::Spell, 56), &t, false, None), Some(492.0));
        // Not the first target: no Saboteur.
        let t = Target::default();
        assert_eq!(predict(&c, row(Kind::Spell, 56), &t, false, None), Some(276.0));
    }

    /// Haste: 180, Composure x3 on ourselves only; Perpetuance x2 (white); a level-50 party member gets 50/96 of Haste
    /// II.
    #[test]
    fn haste() {
        let mut c = rdm99();
        let me = Target { me: true, ..Default::default() };
        let other = Target::default();
        assert_eq!(predict(&c, row(Kind::Spell, 57), &other, true, None), Some(180.0));
        c.statuses.insert(419);
        assert_eq!(predict(&c, row(Kind::Spell, 57), &me, true, None), Some(540.0));
        assert_eq!(predict(&c, row(Kind::Spell, 57), &other, true, None), Some(180.0));
        c.statuses.insert(469);
        assert_eq!(predict(&c, row(Kind::Spell, 57), &other, true, None), Some(360.0));
        let low = Target { level: Some(48), ..Default::default() };
        assert_eq!(predict(&c, row(Kind::Spell, 511), &low, true, None), Some(180.0));
    }

    /// Spec 30.2: song+ 3, no abilities: floor(120 x 1.3) = 156; Troubadour 312.
    #[test]
    fn songs() {
        let mut c = Caster { main: (10, 75), ..Default::default() };
        c.mods.insert(434, 2.0);
        c.mods.insert(452, 1.0);
        assert_eq!(predict(&c, row(Kind::Spell, 398), &Target::default(), false, None), Some(156.0));
        c.statuses.insert(348);
        assert_eq!(predict(&c, row(Kind::Spell, 398), &Target::default(), false, None), Some(312.0));
    }

    /// Steps: 60 new, then the time left + 30 up to 120.
    #[test]
    fn steps() {
        let c = Caster { main: (19, 75), ..Default::default() };
        let r = row(Kind::Ability, 201);
        assert_eq!(predict(&c, r, &Target::default(), false, None), Some(60.0));
        assert_eq!(predict(&c, r, &Target::default(), false, Some(45.5)), Some(75.0));
        assert_eq!(predict(&c, r, &Target::default(), false, Some(100.0)), Some(120.0));
    }

    /// Rolls: 300 + 20 per Winning Streak merit (COR main 75).
    #[test]
    fn rolls() {
        let mut c = Caster { main: (17, 75), ..Default::default() };
        c.merits.insert(3076, 2);
        assert_eq!(predict(&c, row(Kind::Ability, 105), &Target::default(), false, None), Some(340.0));
        assert!(lands(row(Kind::Ability, 105), 421, 11));
        assert!(!lands(row(Kind::Ability, 105), 421, 0));
    }

    /// Landing checks: Dia on any of its nuke messages; a buff only with its own effect in the value.
    #[test]
    fn landing() {
        assert!(lands(row(Kind::Spell, 23), 2, 37));
        assert!(lands(row(Kind::Spell, 23), 85, 0));
        assert!(lands(row(Kind::Spell, 57), 230, 33));
        assert!(!lands(row(Kind::Spell, 57), 230, 0));
        assert!(!lands(row(Kind::Spell, 57), 75, 0));
        assert!(icon_matches(66, 445) && icon_matches(19, 2) && !icon_matches(33, 2));
        // Burn removes Frost; Dia ends our Bio.
        assert!(ends(128).contains(&129));
        assert!(ends(134).contains(&135));
    }

    /// Weapon skills by TP (spec 70.9): Armor Break 120 + 6 TP / 100, Full Break's TP term, Blade: Retsu's fTP.
    #[test]
    fn weapon_skills() {
        let mut c = Caster { main: (1, 75), tp: 2000.0, ..Default::default() };
        assert_eq!(predict(&c, row(Kind::Ws, 83), &Target::default(), false, None), Some(240.0));
        c.tp = 3000.0;
        assert_eq!(predict(&c, row(Kind::Ws, 87), &Target::default(), false, None), Some(150.0));
        c.tp = 1500.0;
        assert_eq!(predict(&c, row(Kind::Ws, 129), &Target::default(), false, None), Some(45.0));
    }

    /// Avatar wards add the Summoning skill over 300 (at most 200); Perfect Defense 30 + skill / 20.
    #[test]
    fn wards() {
        let mut c = Caster { main: (15, 75), ..Default::default() };
        c.skills.insert(38, 420.0);
        assert_eq!(predict(&c, row(Kind::Pet, 595), &Target::default(), false, None), Some(300.0));
        c.skills.insert(38, 600.0);
        assert_eq!(predict(&c, row(Kind::Pet, 595), &Target::default(), false, None), Some(380.0));
        assert_eq!(predict(&c, row(Kind::Pet, 671), &Target::default(), false, None), Some(60.0));
    }

    /// Dancer: Chocobo Jig II (120 + JP) x (1 + mod %, at most 50); a samba (120 + mod + 2 JP) x (100 + pmod) / 100.
    #[test]
    fn dancer() {
        let mut c = Caster { main: (19, 99), ..Default::default() };
        c.jps.insert(613, 10);
        c.mods.insert(492, 60.0);
        assert_eq!(predict(&c, row(Kind::Ability, 381), &Target::default(), false, None), Some(195.0));
        c.mods.insert(490, 10.0);
        c.mods.insert(498, 10.0);
        assert_eq!(predict(&c, row(Kind::Ability, 189), &Target { me: true, ..Default::default() }, false, None), Some(143.0));
    }

    /// Helix: 30, +60 from main level 60, + HELIX_DURATION; Indi on an Entrust target 180 + INDI_DURATION.
    #[test]
    fn helix_and_indi() {
        let mut c = Caster { main: (20, 75), ..Default::default() };
        c.mods.insert(477, 72.0);
        assert_eq!(predict(&c, row(Kind::Spell, 278), &Target::default(), false, None), Some(162.0));
        c.mods.insert(960, 21.0);
        assert_eq!(predict(&c, row(Kind::Spell, 768), &Target::default(), false, None), Some(201.0));
    }

    /// Merits count only for the main job at 75 (Winning Streak is COR's).
    #[test]
    fn merits() {
        let mut c = Caster { main: (1, 75), ..Default::default() };
        c.merits.insert(3076, 5);
        assert_eq!(c.merit(3076), 0.0);
        c.main = (17, 74);
        assert_eq!(c.merit(3076), 0.0);
        c.main = (17, 75);
        assert_eq!(c.merit(3076), 100.0);
    }
}
