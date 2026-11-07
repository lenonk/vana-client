//! Base ability recasts. Not the client's: the ability DAT has none, and s2c 0x119 gives only the seconds left. The
//! bundled `data/recasts.txt` is LandSandBoat's (data/recast_gen.py): before gear, merits and job points.

use std::collections::HashMap;
use std::sync::OnceLock;

const BUNDLED: &str = include_str!("../../data/recasts.txt");

struct Table {
    /// The client's ability id (0x200 + LSB's) -> seconds.
    by_ability: HashMap<u16, u32>,
    /// A 0x119 timer -> the longest of its abilities' seconds.
    by_timer: HashMap<u16, u32>,
}

fn table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = Table { by_ability: HashMap::new(), by_timer: HashMap::new() };
        for l in BUNDLED.lines().filter(|l| !l.starts_with('#')) {
            let mut f = l.split(' ').map(str::parse::<u32>);
            let (Some(Ok(id)), Some(Ok(timer)), Some(Ok(secs))) = (f.next(), f.next(), f.next()) else { continue };
            t.by_ability.insert(0x200 + id as u16, secs);
            let e = t.by_timer.entry(timer as u16).or_default();
            *e = (*e).max(secs);
        }
        t
    })
}

/// An ability's base recast in seconds, by the client's ability id.
pub fn ability(id: u16) -> Option<u32> {
    table().by_ability.get(&id).copied()
}

/// A 0x119 timer's base recast in seconds.
pub fn timer(id: u16) -> Option<u32> {
    table().by_timer.get(&id).copied()
}

#[cfg(test)]
mod tests {
    #[test]
    fn known_recasts() {
        // Provoke (LSB 35, timer 5, 30 s); Jump (66, timer 158, 60 s); the Ready / Sic timer 102 takes Sic's 90 s.
        assert_eq!(super::ability(0x200 + 35), Some(30));
        assert_eq!(super::timer(5), Some(30));
        assert_eq!(super::ability(0x200 + 66), Some(60));
        assert_eq!(super::timer(102), Some(90));
        assert_eq!(super::ability(1), None);
    }
}
