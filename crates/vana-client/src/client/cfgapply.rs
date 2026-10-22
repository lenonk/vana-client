//! What the config ids do in the world, run on every change and once at login (registering a callback calls
//! it with the current value, FUN_1018e110). Ids read where they are used (camera, keys, filters, colours)
//! need nothing here.

use super::World;
use crate::gamecfg::{TABLE, Values};

/// All ids, as at login.
pub fn apply_all(w: &mut World) {
    normalise(&mut w.game);
    for d in TABLE {
        apply(w, d.id);
    }
}

/// Window 2's -1 width and resize time follow window 1 (callbacks of 0xc5 / 0xc6).
pub fn normalise(v: &mut Values) {
    for (w2, w1) in [(0xc5, 0x9c), (0xc6, 0x11)] {
        if v.get_i(w2) == -1 {
            v.set(w2, v.get(w1));
        }
    }
}

pub fn apply(w: &mut World, id: u16) {
    match id {
        // LAB_101a2c60: *(DAT_10565d34 + 0x51) = (v == 0).
        7 => w.auto_lock = w.game.get(7) == 0,
        // FUN_1015c180 .. FUN_1015c220 on log window 1.
        0x0e | 0x0f | 0x10 | 0x11 | 0x9c => w.log.cfg = super::logwin::LogCfg::from_config(&w.game),
        // SE / music level (FUN_10035060 / FUN_10036480): the app sets the mixer.
        9 | 10 => w.volume_dirty = true,
        _ => {}
    }
}

/// Config 0x49 focal length for the field of view.
pub fn focal(v: &Values) -> f32 {
    v.get_i(0x49).clamp(242, 902) as f32
}

/// A chat line's colour by its kind (s2c 0x017 / our sends): say 0x1b, shout 0x1c, tell 0x1d, party 0x1e,
/// linkshell 0x1f, emote 0x20, yell 0x9b, linkshell 2 0xb3, unity 0xb0.
pub fn chat_colour(v: &Values, kind: u8) -> [u8; 4] {
    let id = match kind {
        1 => 0x1c,
        3 => 0x1d,
        4 => 0x1e,
        5 => 0x1f,
        7 | 8 => 0x20,
        0x1a => 0x9b,
        0x1b => 0xb3,
        0x21 => 0xb0,
        _ => 0x1b,
    };
    crate::dat::battle::argb_bytes(v.get(id))
}

/// Timestamp for chat lines (0xb5: 1 hh:mm, 2 hh:mm:ss; English is 12-hour, FUN_101bf740) in local time.
pub fn timestamp(v: &Values) -> Option<String> {
    let mode = v.get(0xb5);
    if mode == 0 {
        return None;
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64) + utc_offset();
    let (h, m, s) = ((secs / 3600).rem_euclid(24), (secs / 60).rem_euclid(60), secs.rem_euclid(60));
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    Some(if mode == 1 { format!("[{h12:02}:{m:02}]") } else { format!("[{h12:02}:{m:02}:{s:02}]") })
}

/// The local UTC offset in seconds, from date(1) once.
fn utc_offset() -> i64 {
    static OFF: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| {
        let out = std::process::Command::new("date").arg("+%z").output().ok();
        let s = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
        let (sign, digits) = match s.strip_prefix('-') {
            Some(d) => (-1, d),
            None => (1, s.strip_prefix('+').unwrap_or(&s)),
        };
        let n: i64 = digits.parse().unwrap_or(0);
        sign * ((n / 100) * 3600 + (n % 100) * 60)
    })
}

/// The projection aspect: the window's, or with `retail` the chosen ratio (0x43 in game, 0xab in the lobby)
/// as FUN_100147a0 builds it: 1 / ((h / (0.75 w)) * 0.791).
pub fn aspect(window: [u32; 2], retail: bool, ratio: u32) -> f32 {
    if !retail {
        return window[0] as f32 / window[1].max(1) as f32;
    }
    let r = super::configui::ASPECTS.get(ratio as usize).map_or(4.0 / 3.0, |a| a.1);
    0.75 * r / 0.791
}

/// Movement and turn keys from the k1assign ids (0x4a..0x4d move up / down / left / right, 0x66 / 0x67 turn
/// right / left), as winit keys.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_aspect() {
        let v = Values::default();
        assert_eq!(focal(&v), 350.0);
        assert_eq!(chat_colour(&v, 3), [0xa0, 0x40, 0xa0, 0x80]);
        assert!(timestamp(&v).is_none());
        assert!((aspect([1600, 900], false, 0) - 16.0 / 9.0).abs() < 1e-6);
        assert!((aspect([1600, 900], true, 0) - 1.0 / 0.791).abs() < 1e-5);
    }
}
