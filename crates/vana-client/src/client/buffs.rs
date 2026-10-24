//! Our status icons: "menu    buff" (object DAT_106671b8, update FUN_10227bf0, icon / label callbacks LAB_10227f70 /
//! LAB_10228080): open in the field, out of battle, engaged and in events; 9 cells a row at the layout's children.

use super::World;
use super::game_ui::GameUi;

const WINDOW: &str = "menu    buff";

/// FUN_10227bf0 step 2: the 0x037 ids in order, 0xffff and 0 (KO) skipped, each with its 0x063 end time when the
/// timer copy has the same id at that index, else 0x7fffffff.
fn icons(w: &World) -> Vec<(u16, u32)> {
    (0..32)
        .filter_map(|i| {
            let id = w.buffs[i];
            if id == 0xffff || id == 0 {
                return None;
            }
            let end = w.buff_timers.as_ref().filter(|t| t.0[i] == id).map_or(0x7fff_ffff, |t| t.1[i]);
            Some((id, end))
        })
        .collect()
}

/// LAB_10228080's label: "%dh" from an hour (at most 99), "%dm" from a minute, seconds below; none at 5 s or less.
fn label(rem: i32) -> Option<String> {
    let rem = rem.min(356_400);
    match rem {
        r if r <= 5 => None,
        r if r >= 3600 => Some(format!("{}h", r / 3600)),
        r if r >= 60 => Some(format!("{}m", r / 60)),
        r => Some(r.to_string()),
    }
}

/// The icons at cell + 1 (24x24: 32 px x 0.75, or 16 px x 1.5 with config 0x99 bit 2), dimming to half and back
/// every 40 ticks in their last 15 s; the time under each in fontshp x0.85 (config 0xc3).
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) {
    let list = icons(w);
    if list.is_empty() {
        return;
    }
    let Some((x0, y0)) = gu.window_xy(WINDOW, sw, sh) else { return };
    let small = w.game.get(0x99) & 4 != 0;
    let timers = w.game.get(0xc3) != 0;
    let now = (super::vana_now(w) as u32).wrapping_mul(60);
    // +0x28c counts 40 ticks down; +0x288 the triangle 0..1 of it.
    let t = 40.0 - (w.started.elapsed().as_secs_f32() * 60.0) % 40.0;
    let phase = if t <= 20.0 { t } else { 40.0 - t } * 0.05;
    for (k, (id, end)) in list.into_iter().take(36).enumerate() {
        let (cx, cy) = (x0 + 2.0 + 26.0 * (k % 9) as f32, y0 + 2.0 + 26.0 * (k / 9) as f32);
        let Some(tex) = gu.status_icon(small, id) else { continue };
        let timed = timers && end != 0x7fff_ffff && !gu.status_entry(small, id).is_some_and(|e| e.no_timer);
        let rem = (end.wrapping_sub(now).wrapping_add(59) as i32) / 60;
        let c = if timed && rem <= 15 { (0x40 + (64.0 * phase) as u32) as f32 / 128.0 } else { 1.0 };
        gu.image_rect(&tex, cx + 1.0, cy + 1.0, 24.0, 24.0, [c, c, c, 1.0]);
        if let Some(s) = label(rem).filter(|_| timed) {
            gu.shape_scaled_centred(s.as_bytes(), cx + 14.0, cy + 20.0, 0.85, 0x8080_8080);
        }
    }
}

#[cfg(test)]
mod tests {
    /// The user's retail labels: 25m, 1m, 5h, 2m, 16; 1h59m is 1h; none at 5 s.
    #[test]
    fn labels() {
        assert_eq!([1500, 60, 18000, 150, 16, 7140, 5, 400_000].map(super::label), [Some("25m".into()), Some("1m".into()), Some("5h".into()), Some("2m".into()), Some("16".into()), Some("1h".into()), None, Some("99h".into())]);
    }
}
