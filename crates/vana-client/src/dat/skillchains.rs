//! Skillchain data. Not the client's: the server applies the properties, so the game files hold no table of them.
//! The bundled `data/skillchains.txt` comes from LandSandBoat's SQL (`vana-client skillchain-gen`): weapon skill, mob
//! skill (trusts, pets) and blue magic properties and the trusts' skill lists. The chain rules and elements are
//! battleutils.cpp's (`skillchain_map`, FormSkillchain, GetSkillchainMagicElement).

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

const BUNDLED: &str = include_str!("../../data/skillchains.txt");

/// A skillchain property: LSB's SKILLCHAIN_ELEMENT (1 Transfixion .. 8 Impaction level 1, 9..12 level 2, 13 / 14 Light
/// / Darkness, 15 / 16 their second level). 0 none.
pub type Prop = u8;

pub const NAMES: [&str; 17] = [
    "",
    "Transfixion",
    "Compression",
    "Liquefaction",
    "Scission",
    "Reverberation",
    "Detonation",
    "Induration",
    "Impaction",
    "Gravitation",
    "Distortion",
    "Fusion",
    "Fragmentation",
    "Light",
    "Darkness",
    "Light",
    "Darkness",
];

/// The elements a magic burst can use, in GetSkillchainMagicElement's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Element {
    Fire,
    Ice,
    Wind,
    Earth,
    Thunder,
    Water,
    Light,
    Dark,
}

impl Element {
    pub fn name(self) -> &'static str {
        match self {
            Element::Fire => "Fire",
            Element::Ice => "Ice",
            Element::Wind => "Wind",
            Element::Earth => "Earth",
            Element::Thunder => "Thunder",
            Element::Water => "Water",
            Element::Light => "Light",
            Element::Dark => "Dark",
        }
    }

    /// The spell table's element (+0x0a: 0 fire .. 7 dark).
    pub fn of_spell(e: u8) -> Option<Element> {
        use Element::*;
        [Fire, Ice, Wind, Earth, Thunder, Water, Light, Dark].get(e as usize).copied()
    }
}

pub fn elements(p: Prop) -> &'static [Element] {
    use Element::*;
    match p {
        1 => &[Light],
        2 => &[Dark],
        3 => &[Fire],
        4 => &[Earth],
        5 => &[Water],
        6 => &[Wind],
        7 => &[Ice],
        8 => &[Thunder],
        9 => &[Dark, Earth],
        10 => &[Water, Ice],
        11 => &[Light, Fire],
        12 => &[Wind, Thunder],
        13 | 15 => &[Light, Fire, Wind, Thunder],
        14 | 16 => &[Dark, Earth, Water, Ice],
        _ => &[],
    }
}

/// GetSkillchainTier: 1 for the level-1 properties, 2, 3 (Light / Darkness), 4.
/// Light / Darkness of either level as one (their names are the same): 15 -> 13, 16 -> 14.
pub fn family(p: Prop) -> Prop {
    match p {
        15 => 13,
        16 => 14,
        p => p,
    }
}

pub fn level(p: Prop) -> u8 {
    match p {
        1..=8 => 1,
        9..=12 => 2,
        13 | 14 => 3,
        15 | 16 => 4,
        _ => 0,
    }
}

/// `skillchain_map`: (the closing skill's property, the resonance) -> the chain.
const MAP: [((Prop, Prop), Prop); 30] = [
    ((13, 13), 15),
    ((14, 14), 16),
    ((10, 9), 14),
    ((12, 9), 12),
    ((9, 10), 14),
    ((11, 10), 11),
    ((9, 11), 9),
    ((12, 11), 13),
    ((10, 12), 10),
    ((11, 12), 13),
    ((4, 1), 10),
    ((8, 3), 11),
    ((2, 6), 9),
    ((5, 7), 12),
    ((2, 1), 2),
    ((5, 1), 5),
    ((1, 2), 1),
    ((6, 2), 6),
    ((4, 3), 4),
    ((3, 4), 3),
    ((5, 4), 5),
    ((6, 4), 6),
    ((7, 5), 7),
    ((8, 5), 8),
    ((4, 6), 4),
    ((2, 7), 2),
    ((8, 7), 8),
    ((3, 8), 3),
    ((6, 8), 6),
    ((0, 0), 0),
];

/// FormSkillchain: each resonance property in order against each of the closer's; the first pair in the map wins.
pub fn form(resonance: &[Prop], closer: &[Prop]) -> Option<Prop> {
    resonance.iter().filter(|&&r| r != 0).find_map(|&r| closer.iter().filter(|&&c| c != 0).find_map(|&c| MAP.iter().find(|(k, _)| *k == (c, r)).map(|(_, v)| *v)))
}

/// What uses a skill: a weapon skill (category 3), a mob or pet skill (category 11 / 13), blue magic (category 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Skill {
    Weapon(u16),
    Mob(u16),
    Blue(u16),
}

#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub props: [Prop; 3],
    /// LSB's name ("blade_rin"), for skills the client has no name table for.
    pub name: String,
    /// Weapon skills: the skill level that unlocks it (a rough power order).
    pub skill: u16,
}

#[derive(Default)]
pub struct Data {
    pub weapon: HashMap<u16, Entry>,
    pub mob: HashMap<u16, Entry>,
    pub blue: HashMap<u16, Entry>,
    /// Trust (name folded by `fold`) -> its skills (mob skill ids).
    pub trusts: HashMap<String, Vec<u16>>,
}

impl Data {
    pub fn get(&self, s: Skill) -> Option<&Entry> {
        match s {
            Skill::Weapon(id) => self.weapon.get(&id),
            Skill::Mob(id) => self.mob.get(&id).or_else(|| self.weapon.get(&id).filter(|_| id < 256)),
            Skill::Blue(id) => self.blue.get(&id),
        }
    }

    pub fn props(&self, s: Skill) -> [Prop; 3] {
        self.get(s).map_or([0; 3], |e| e.props)
    }

    /// A trust's skills by its party name.
    pub fn trust(&self, name: &str) -> Option<&[u16]> {
        self.trusts.get(&fold(name)).map(Vec::as_slice)
    }
}

/// Lower case, letters and digits only: "TRUST_Ayame_UC" and "Ayame (UC)" both give "ayameuc".
pub fn fold(name: &str) -> String {
    name.trim_start_matches("TRUST_").chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

/// "red_lotus_blade" -> "Red Lotus Blade".
pub fn pretty(name: &str) -> String {
    name.split('_').filter(|w| !w.is_empty()).map(|w| {
        let mut c = w.chars();
        c.next().map_or(String::new(), |f| f.to_ascii_uppercase().to_string() + c.as_str())
    }).collect::<Vec<_>>().join(" ")
}

pub fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| parse(BUNDLED))
}

fn parse(text: &str) -> Data {
    let mut d = Data::default();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split_whitespace().collect();
        let num = |k: usize| f.get(k).and_then(|v| v.parse::<u16>().ok()).unwrap_or(0);
        let props = |k: usize| [num(k) as u8, num(k + 1) as u8, num(k + 2) as u8];
        match f.first() {
            Some(&"w") => {
                d.weapon.insert(num(1), Entry { props: props(2), skill: num(6), name: f.get(7).unwrap_or(&"").to_string() });
            }
            Some(&"m") => {
                d.mob.insert(num(1), Entry { props: props(2), name: f.get(5).unwrap_or(&"").to_string(), ..Default::default() });
            }
            Some(&"b") => {
                d.blue.insert(num(1), Entry { props: props(2), ..Default::default() });
            }
            Some(&"t") => {
                d.trusts.insert(f.get(1).unwrap_or(&"").to_string(), f[2..].iter().filter_map(|v| v.parse().ok()).collect());
            }
            _ => {}
        }
    }
    d
}

/// The values of each `INSERT INTO <table> VALUES (...)` line (quotes stripped, `@VARS` kept as text).
fn rows(sql: &str, table: &str) -> Vec<Vec<String>> {
    let head = format!("INSERT INTO `{table}` VALUES (");
    sql.lines()
        .filter_map(|l| l.trim_start().strip_prefix(&head))
        .map(|l| {
            let mut out = Vec::new();
            let (mut cur, mut quoted, mut prev) = (String::new(), false, ' ');
            for c in l.chars() {
                match c {
                    '\'' if prev != '\\' => quoted = !quoted,
                    ',' if !quoted => out.push(std::mem::take(&mut cur).trim().to_string()),
                    ')' if !quoted => {
                        out.push(std::mem::take(&mut cur).trim().to_string());
                        break;
                    }
                    _ => cur.push(c),
                }
                prev = c;
            }
            out
        })
        .collect()
}

/// `skillchain-gen`: data/skillchains.txt from LSB's sql/ tables.
pub fn generate(lsb: &Path, out: &Path) -> Result<()> {
    let read = |name: &str| std::fs::read_to_string(lsb.join("sql").join(name)).with_context(|| format!("reading {name}"));
    let n = |v: &str| v.parse::<i64>().unwrap_or(0).max(0);
    let mut text = String::from("# vana-client skillchain-gen from LandSandBoat sql/: weapon_skills, mob_skills, pet_skills, blue_spell_list, mob_skill_lists\n");
    text += "# w id p1 p2 p3 type skilllevel name / m id p1 p2 p3 name / b spell p1 p2 p3 / t trust skill...\n";
    for r in rows(&read("weapon_skills.sql")?, "weapon_skills") {
        // weaponskillid, name, jobs, type, skilllevel, element, animation, animationTime, range, aoe, radius, primary_sc..
        if r.len() >= 14 {
            text += &format!("w {} {} {} {} {} {} {}\n", n(&r[0]), n(&r[11]), n(&r[12]), n(&r[13]), n(&r[3]), n(&r[4]), r[1]);
        }
    }
    let mut mob = std::collections::BTreeMap::new();
    for r in rows(&read("mob_skills.sql")?, "mob_skills") {
        if r.len() >= 15 && (n(&r[12]) | n(&r[13]) | n(&r[14])) != 0 {
            mob.insert(n(&r[0]), format!("{} {} {} {}", n(&r[12]), n(&r[13]), n(&r[14]), r[2]));
        }
    }
    // pet_skill_id, mob_skill_id, anim, name, aoe, radius, distance, anim time, prepare, targets, message, flag, param,
    // finish category, knockback, primary_sc..: keyed by the pet skill id the action carries.
    for r in rows(&read("pet_skills.sql")?, "pet_skills") {
        if r.len() >= 18 && (n(&r[15]) | n(&r[16]) | n(&r[17])) != 0 {
            mob.entry(n(&r[0])).or_insert_with(|| format!("{} {} {} {}", n(&r[15]), n(&r[16]), n(&r[17]), r[3]));
        }
    }
    for (id, rest) in mob {
        text += &format!("m {id} {rest}\n");
    }
    for r in rows(&read("blue_spell_list.sql")?, "blue_spell_list") {
        if r.len() >= 8 && (n(&r[5]) | n(&r[6]) | n(&r[7])) != 0 {
            text += &format!("b {} {} {} {}\n", n(&r[0]), n(&r[5]), n(&r[6]), n(&r[7]));
        }
    }
    let mut trusts: std::collections::BTreeMap<String, Vec<i64>> = Default::default();
    for r in rows(&read("mob_skill_lists.sql")?, "mob_skill_lists") {
        if r.len() >= 3 && r[0].starts_with("TRUST_") && n(&r[2]) != 0 {
            trusts.entry(fold(&r[0])).or_default().push(n(&r[2]));
        }
    }
    for (name, ids) in trusts {
        text += &format!("t {name} {}\n", ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(" "));
    }
    std::fs::write(out, text).with_context(|| format!("writing {}", out.display()))?;
    println!("wrote {}", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chains() {
        // Liquefaction then Impaction: Fusion; Fusion then Fragmentation: Light; Light then Light: Light II.
        assert_eq!(form(&[3], &[8]), Some(11));
        assert_eq!(form(&[11], &[12]), Some(13));
        assert_eq!(form(&[13], &[13]), Some(15));
        // The resonance's first property that pairs wins: Transfixion first gives Distortion with Scission.
        assert_eq!(form(&[1, 3], &[4]), Some(10));
        assert_eq!(form(&[3, 1], &[4]), Some(4));
        assert_eq!(form(&[1], &[1]), None);
    }

    #[test]
    fn bundled() {
        let d = data();
        // Blade: Rin (128) Transfixion; Blade: Jin (134) Impaction / Detonation.
        assert_eq!(d.weapon.get(&128).map(|e| e.props), Some([1, 0, 0]));
        assert_eq!(d.weapon.get(&134).map(|e| e.props[..2].to_vec()), Some(vec![6, 8]));
        assert!(d.trust("Ayame (UC)").is_some() || d.trust("Ayame").is_some());
        assert_eq!(fold("D. Shantotto"), "dshantotto");
    }
}
