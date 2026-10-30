//! Ours (Extras, not retail; the Battlemod add-on's idea): battle lines in a short form and names coloured by who
//! they are. "Lnxcsfix uses Fast Blade. / The Wild Rabbit takes 123 points of damage." reads "Lnxcsfix [Fast Blade] →
//! Wild Rabbit: 123". The retail text is read back with the actor's and target's names as they print; a message whose
//! lines match none of the forms below stays as retail prints it.

use crate::dat::battle::Who;
use crate::dat::font;

/// The in-line colour codes (`1e xx`, table 0x103353c8) a name takes.
pub mod colour {
    pub const ME: u8 = 7;
    pub const PARTY: u8 = 6;
    pub const ALLIANCE: u8 = 3;
    /// A monster our party claimed / anyone else's claim.
    pub const OUR_CLAIM: u8 = 8;
    pub const OTHER_CLAIM: u8 = 5;
}

/// A name in a short line: no article, in its colour when it has one.
fn name(w: &Who) -> String {
    if w.colour == 0 {
        w.name.clone()
    } else {
        format!("{}{}{}", font::colour_code(w.colour), w.name, font::colour_code(1))
    }
}

/// How a name prints in the retail text, longest first ("The Wild Rabbit", "the Wild Rabbit", "Wild Rabbit").
fn forms(w: &Who) -> Vec<String> {
    if w.pc || w.named { vec![w.name.clone()] } else { vec![format!("The {}", w.name), format!("the {}", w.name), w.name.clone()] }
}

/// `line` with the first printed form of `w` as `token`.
fn mark(line: &str, w: Option<&Who>, token: &str) -> String {
    let Some(w) = w else { return line.to_string() };
    forms(w).iter().find(|f| line.contains(f.as_str())).map_or_else(|| line.to_string(), |f| line.replacen(f.as_str(), token, 1))
}

/// A status name as the result shows it: its first letter capitalised ("weight" -> "Weight").
fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// What a line about the target says, short ("123", "+45 HP", "+Protect", "resisted").
fn result(line: &str) -> Option<String> {
    let (burst, line) = match line.strip_prefix("Magic Burst! ") {
        Some(l) => (true, l),
        None => (false, line),
    };
    let r = if let Some(n) = line.strip_prefix("{T} takes ").and_then(|l| l.strip_suffix(" points of damage.")) {
        n.to_string()
    } else if let Some(n) = line.strip_prefix("{T} takes ").and_then(|l| l.strip_suffix(" point of damage.")) {
        n.to_string()
    } else if let Some(n) = line.strip_prefix("{T} recovers ").and_then(|l| l.strip_suffix(" HP.")) {
        format!("+{n} HP")
    } else if let Some(n) = line.strip_prefix("{T} recovers ").and_then(|l| l.strip_suffix(" MP.")) {
        format!("+{n} MP")
    } else if let Some(e) = line.strip_prefix("{T} gains the effect of ").or_else(|| line.strip_prefix("{T} receives the effect of ")).and_then(|l| l.strip_suffix('.')) {
        format!("+{}", cap(e))
    } else if let Some(e) = line.strip_prefix("{T}'s ").and_then(|l| l.strip_suffix(" effect wears off.")) {
        format!("-{}", cap(e))
    } else if let Some(n) = line.strip_suffix(" HP drained from {T}.") {
        format!("{n} HP drained")
    } else if let Some(n) = line.strip_suffix(" MP drained from {T}.") {
        format!("{n} MP drained")
    } else if matches!(line, "{T} resists the spell." | "{T} resists the effects of the spell!") {
        "resisted".into()
    } else if line == "No effect on {T}." {
        "no effect".into()
    } else if line == "{T} evades." {
        "evaded".into()
    } else {
        return None;
    };
    Some(if burst { format!("{r} (Magic Burst)") } else { r })
}

/// The message's retail lines (codes kept) as one short line, or None to print them as they are.
pub fn shorten(lines: &[String], actor: Option<&Who>, target: Option<&Who>) -> Option<String> {
    let plain: Vec<String> = lines.iter().map(|l| font::strip_codes(l).trim().to_string()).filter(|l| !l.is_empty()).collect();
    let (a, t) = (actor.map(name), target.map(name));
    // The actor is named first in the opening line, the target after it.
    let first = plain.first().map(|l| mark(&mark(l, actor, "{A}"), target, "{T}"))?;
    let rest: Vec<String> = plain[1..].iter().map(|l| mark(&mark(l, target, "{T}"), actor, "{A}")).collect();
    let short = |act: Option<&str>, r: &str| -> Option<String> {
        let (a, t) = (a.clone()?, t.clone()?);
        Some(match act {
            Some(x) => format!("{a} [{x}] → {t}: {r}"),
            None => format!("{a} → {t}: {r}"),
        })
    };
    match rest.as_slice() {
        [] => {
            if let Some(n) = first.strip_prefix("{A} hits {T} for ").and_then(|l| l.strip_suffix(" points of damage.").or_else(|| l.strip_suffix(" point of damage."))) {
                return short(None, n);
            }
            if first == "{A} misses {T}." {
                return short(None, "miss");
            }
            if let Some(x) = first.strip_prefix("{A} uses ").and_then(|l| l.strip_suffix(", but misses {T}.")) {
                return short(Some(x), "miss");
            }
            // "Lnxtest uses Provoke on the Snipper." -> "Lnxtest [Provoke] → Snipper".
            if let Some(x) = first.strip_prefix("{A} uses ").and_then(|l| l.strip_suffix(" on {T}.")).filter(|x| !x.contains('{')) {
                return Some(format!("{} [{x}] → {}", a.clone()?, t.clone()?));
            }
            // A line about the target alone (an area action's later targets, a wear-off).
            result(&first).and_then(|r| Some(format!("{}: {r}", t.clone()?)))
        }
        [second] => {
            let r = result(second)?;
            if first == "{A} scores a critical hit!" {
                return short(None, &format!("{r}!"));
            }
            if let Some(sc) = first.strip_prefix("Skillchain: ").and_then(|l| l.strip_suffix('.')) {
                return Some(format!("[Skillchain: {sc}] → {}: {r}", t.clone()?));
            }
            let act = first.strip_prefix("{A} uses ").or_else(|| first.strip_prefix("{A} casts ")).and_then(|l| l.strip_suffix('.'))?;
            let act = act.strip_suffix(" on {T}").unwrap_or(act);
            (!act.contains('{')).then(|| short(Some(act), &r)).flatten()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who(n: &str, pc: bool, colour: u8) -> Who {
        Who { name: n.into(), pc, colour, ..Default::default() }
    }

    /// The common forms; an unknown one stays retail; names coloured with their codes.
    #[test]
    fn short_lines() {
        let me = who("Lnxcsfix", true, 0);
        let rabbit = who("Wild Rabbit", false, 0);
        let s = |l: &[&str]| shorten(&l.iter().map(|x| x.to_string()).collect::<Vec<_>>(), Some(&me), Some(&rabbit));
        assert_eq!(s(&["Lnxcsfix uses Fast Blade.", "The Wild Rabbit takes 123 points of damage."]).as_deref(), Some("Lnxcsfix [Fast Blade] → Wild Rabbit: 123"));
        assert_eq!(s(&["Lnxcsfix hits the Wild Rabbit for 18 points of damage."]).as_deref(), Some("Lnxcsfix → Wild Rabbit: 18"));
        assert_eq!(s(&["Lnxcsfix scores a critical hit!", "The Wild Rabbit takes 31 points of damage."]).as_deref(), Some("Lnxcsfix → Wild Rabbit: 31!"));
        assert_eq!(s(&["Lnxcsfix misses the Wild Rabbit."]).as_deref(), Some("Lnxcsfix → Wild Rabbit: miss"));
        assert_eq!(s(&["Lnxcsfix casts Dia.", "The Wild Rabbit gains the effect of dia."]).as_deref(), Some("Lnxcsfix [Dia] → Wild Rabbit: +Dia"));
        assert_eq!(s(&["Lnxcsfix casts Fire.", "Magic Burst! The Wild Rabbit takes 80 points of damage."]).as_deref(), Some("Lnxcsfix [Fire] → Wild Rabbit: 80 (Magic Burst)"));
        assert_eq!(s(&["The Wild Rabbit takes 48 points of damage."]).as_deref(), Some("Wild Rabbit: 48"));
        assert_eq!(s(&["Lnxcsfix defeats the Wild Rabbit."]), None);
        assert_eq!(s(&["Lnxcsfix uses Provoke on the Wild Rabbit."]).as_deref(), Some("Lnxcsfix [Provoke] → Wild Rabbit"));
        let heal = shorten(&["Lnxcsfix casts Cure.".into(), "Lnxcsfix recovers 45 HP.".into()], Some(&me), Some(&me));
        assert_eq!(heal.as_deref(), Some("Lnxcsfix [Cure] → Lnxcsfix: +45 HP"));
        let red = who("Wild Rabbit", false, colour::OUR_CLAIM);
        let c = shorten(&["Lnxcsfix misses the Wild Rabbit.".into()], Some(&me), Some(&red)).unwrap();
        assert_eq!(font::strip_codes(&c), "Lnxcsfix → Wild Rabbit: miss");
        assert!(c.contains(font::colour_code(colour::OUR_CLAIM)));
    }
}
