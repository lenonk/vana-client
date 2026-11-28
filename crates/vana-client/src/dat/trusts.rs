//! `vana-client trust-gen`: party trust job hints for the XIV-UI mod from LandSandBoat `spell_list` + `mob_pools`.
//! Trust spell id `N` uses `mob_pools.poolid = N + 5000` (same join as LSB `trustutils`).

use crate::dat::npc::{NpcLook, parse_look};
use anyhow::{Context, Result, bail};
use std::path::Path;

const TRUST_SPELL_LO: i64 = 896;
const TRUST_SPELL_HI: i64 = 1023;
const POOL_BASE: i64 = 5000;

static JOB: [&str; 23] = [
    "?", "WAR", "MNK", "WHM", "BLM", "RDM", "THF", "PLD", "DRK", "BST", "BRD", "RNG", "SAM", "NIN", "DRG", "SMN",
    "BLU", "COR", "PUP", "DNC", "SCH", "GEO", "RUN",
];

/// One row per trust spell / mob pool (party `packet_name`, main job, entity model id).
#[derive(Clone, Debug, PartialEq, Eq)]
struct TrustRow {
    spell: u16,
    name: String,
    job: String,
    model: u16,
}

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

fn n(v: &str) -> i64 {
    v.parse().unwrap_or(0)
}

fn decode_model(blob: &str) -> Result<u16> {
    let hex = blob.strip_prefix("0x").or_else(|| blob.strip_prefix("0X")).context("modelid blob")?;
    let bytes: Vec<u8> =
        (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16)).collect::<Result<_, _>>()?;
    Ok(match parse_look(&bytes)? {
        NpcLook::Model(m) => m,
        NpcLook::Equipped(_) => bail!("equipped look in trust pool"),
    })
}

fn lua_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

/// Regenerate `modules/party/src/trusts.lua` (or another path) from an LSB checkout.
pub fn generate(lsb: &Path, out: &Path) -> Result<()> {
    let read =
        |name: &str| std::fs::read_to_string(lsb.join("sql").join(name)).with_context(|| format!("reading {name}"));
    let spell_sql = read("spell_list.sql")?;
    let mob_sql = read("mob_pools.sql")?;

    let mut spells = std::collections::BTreeMap::new();
    for r in rows(&spell_sql, "spell_list") {
        let id = n(&r[0]);
        if (TRUST_SPELL_LO..=TRUST_SPELL_HI).contains(&id) && r.len() > 1 {
            spells.insert(id as u16, r[1].clone());
        }
    }

    let mut by_pool = std::collections::BTreeMap::new();
    for r in rows(&mob_sql, "mob_pools") {
        if r.len() < 7 {
            continue;
        }
        let pool = n(&r[0]);
        if pool < TRUST_SPELL_LO + POOL_BASE || pool > TRUST_SPELL_HI + POOL_BASE {
            continue;
        }
        let m_job = n(&r[5]) as usize;
        if m_job >= JOB.len() {
            bail!("pool {pool}: mJob {m_job} out of range");
        }
        let model = decode_model(&r[4])?;
        by_pool.insert(
            pool as u16,
            TrustRow { spell: (pool - POOL_BASE) as u16, name: r[2].clone(), job: JOB[m_job].to_string(), model },
        );
    }

    let mut missing = Vec::new();
    let mut list = Vec::new();
    for spell in TRUST_SPELL_LO..=TRUST_SPELL_HI {
        let pool = (spell + POOL_BASE) as u16;
        match by_pool.get(&pool) {
            Some(row) => list.push(row.clone()),
            None if spells.contains_key(&(spell as u16)) => missing.push(spell),
            None => {}
        }
    }
    if !missing.is_empty() {
        bail!("spell_list trusts without mob_pools row (pool = spell + 5000): {:?}", missing);
    }

    let mut text = String::from("-- Trust party job hints when main_job is 0 (generated; do not edit by hand).\n");
    text.push_str("-- vana-client trust-gen from LandSandBoat sql/spell_list.sql and mob_pools.sql\n");
    text.push_str("-- Regenerate: vana-client trust-gen --lsb <landsandboat-checkout>\n");
    text.push_str("return {\n");
    for row in &list {
        text.push_str(&format!(
            "  {{ name = '{}', job = '{}', model = {}, spell = {} }},\n",
            lua_escape(&row.name),
            row.job,
            row.model,
            row.spell
        ));
    }
    text.push_str("}\n");

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, text).with_context(|| format!("writing {}", out.display()))?;
    println!("wrote {} ({} trusts)", out.display(), list.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shantotto_model_from_lsb_blob() {
        let row = "0x0000B80B00000000000000000000000000000000";
        assert_eq!(decode_model(row).unwrap(), 3000);
    }

    #[test]
    fn parse_mob_pools_insert() {
        let line = "INSERT INTO `mob_pools` VALUES (5896,'shantotto','Shantotto',297,0x0000B80B00000000000000000000000000000000,4,0,2,240,40,0,0,0,0,0,0,32,0,3,0,0,308,0,0,1011,153,1,15);";
        let r = rows(line, "mob_pools");
        assert_eq!(r.len(), 1);
        assert_eq!(n(&r[0][0]), 5896);
        assert_eq!(r[0][2], "Shantotto");
        assert_eq!(decode_model(&r[0][4]).unwrap(), 3000);
        assert_eq!(JOB[n(&r[0][5]) as usize], "BLM");
    }
}
