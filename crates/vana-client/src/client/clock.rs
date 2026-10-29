//! Ours (not retail; the clock add-ons): the Clock extra. Beside the game's Vana'diel clock, Earth's local time in
//! retail's own format (config 0x96 12 / 24 hour), the moon (group 0x11's phase and /clock's percent) and how long
//! until the next Vana'diel day.

use super::World;
use crate::dat::msgfmt;

/// Earth seconds in a Vana'diel day.
const DAY: u64 = 3456;

pub struct Parts {
    /// "20:40" / "8:40 PM".
    pub earth: String,
    /// "Waxing Crescent", its percent.
    pub moon: (String, i32),
    /// The next day's name and the Earth minutes until it.
    pub next: (String, u64),
}

pub fn parts(w: &World) -> Option<Parts> {
    let bt = w.battle.as_ref()?;
    w.clock?;
    let t = super::vana_now(w);
    let p = msgfmt::VanaParts::of(t);
    let g = |id: u16| bt.group10_raw(id as usize);
    let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let earth = crate::dat::font::decode_sjis(&msgfmt::earth_datetime(&g, unix, 8, (w.game.get(0x95) & 3) as usize, w.game.get(0x96) & 1 != 0));
    let next = bt.day_name((p.weekday + 1) % 8).unwrap_or_default();
    Some(Parts { earth: earth.trim().to_string(), moon: (bt.moon_name(p.phase).unwrap_or_default(), p.moon), next: (next, (DAY - t % DAY).div_ceil(60)) })
}

impl Parts {
    /// "Firesday in 12 min" (or "in 1 min").
    pub fn next_text(&self) -> String {
        format!("{} in {} min", self.next.0, self.next.1.max(1))
    }
}
