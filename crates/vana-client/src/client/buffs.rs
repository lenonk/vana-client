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

/// FUN_10222150 for the 9-column list: Left / Right one cell, Up / Down a row (staying when there is none; the wrap
/// rules there are not followed), SE 0001 on a move; Enter (FUN_10227eb0) cancels a cancellable effect: c2s 0x0F1 and
/// the icon goes at once. Escape does nothing here.
pub fn key(gu: &GameUi, w: &mut World, key: egui::Key) {
    let list = icons(w);
    if list.is_empty() {
        w.log_focus = 0;
        return;
    }
    let n = list.len();
    let at = w.buff_cursor.min(n - 1);
    let to = match key {
        egui::Key::ArrowLeft => (at + n - 1) % n,
        egui::Key::ArrowRight => (at + 1) % n,
        egui::Key::ArrowUp if at >= 9 => at - 9,
        egui::Key::ArrowDown if at + 9 < n => at + 9,
        // Escape leaves status select mode (seen in retail; the window's own message 6 does nothing).
        egui::Key::Escape => {
            w.log_focus = 0;
            w.sounds.push(crate::audio::Ui::Cancel as u32);
            return;
        }
        egui::Key::Enter => {
            let id = list[at].0;
            let small = w.game.get(0x99) & 4 != 0;
            if gu.status_entry(small, id).is_some_and(|e| e.cancel) {
                let _ = w.net.commands.send(super::net::NetCommand::CancelStatus(id));
                if let Some(b) = w.buffs.iter_mut().find(|b| **b == id) {
                    *b = 0xffff;
                }
            }
            at
        }
        _ => at,
    };
    if to != at {
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    w.buff_cursor = to;
}

/// While focused: title slot 2 the status name, help slot 2 its description (FUN_10227bf0 step 5).
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    if w.log_focus != 2 {
        return;
    }
    let list = icons(w);
    let Some(&(id, _)) = list.get(w.buff_cursor.min(list.len().saturating_sub(1))) else { return };
    if let Some(bt) = &w.battle {
        bars.title[2] = bt.status_title(id);
    }
    bars.help[2] = gu.status_entry(w.game.get(0x99) & 4 != 0, id).map(|e| e.desc).unwrap_or_default();
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
    if list.is_empty() || !gu.want(WINDOW) {
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
        // FUN_10227970: the window's cursor (its kind-6 ref, the anc_equi box) scaled 0.74 over the icon. Placed on the
        // icon as retail shows it (the user's shot); the (-5, -5) of the hand did not fit the box (inf).
        if w.log_focus == 2 && k == w.buff_cursor {
            gu.cursor_scaled("anc     anc_equi", cx + 1.0, cy + 1.0, 0.74);
        }
    }
}

/// The party icon object (DAT_106671bc, draw FUN_10228440, config 0xc2), while the buff window is up: the focus target
/// ("< name >"), else FUN_10157cb0's (the party cursor's member while it is up, else the current target), each only
/// when someone other than us in our six slots (FUN_101593c0); its 0x076 icons 8 a row at 14 px (24 x 0.6) over the
/// log window (x + 200), or over the party window when they would reach it; the name above in fontshp, grey unless in
/// our zone (FUN_10202cc0).
pub fn draw_party(gu: &mut GameUi, w: &World, sw: f32, sh: f32, log: [f32; 4]) {
    if w.game.get(0xc2) == 0 {
        return;
    }
    let valid = |id: u32| id != w.my_id && super::target::in_own_six(w, id);
    let (id, bracketed) = match super::target::focus_member(w) {
        Some(f) => (f, true),
        None => {
            let t = if w.party_cursor.is_some() { super::target::party_hand(w).and_then(|(_, _, id)| id) } else { w.target };
            match t.filter(|&t| valid(t)) {
                Some(t) => (t, false),
                None => return,
            }
        }
    };
    let Some(row) = w.party_buffs.get(&id) else { return };
    let ids: Vec<u16> = row.iter().copied().filter(|&b| b != 0xffff && b != 0).collect();
    if ids.is_empty() {
        return;
    }
    let rows = (ids.len() - 1) / 8 + 1;
    let (x0, mut y0) = (log[0] + 200.0, log[1] - 8.0 - rows as f32 * 14.0);
    let pw = gu.party_window_name().to_string();
    if let Some(p) = gu.window_rect(&pw, sw, sh)
        && x0 + 144.0 >= p[0]
    {
        y0 = p[1] - 8.0 - rows as f32 * 14.0;
    }
    let small = w.game.get(0x99) & 4 != 0;
    for (j, &icon) in ids.iter().enumerate() {
        let Some(tex) = gu.status_icon(small, icon) else { continue };
        gu.image_rect(&tex, x0 + 1.0 + 14.0 * (j % 8) as f32, y0 + 1.0 + 14.0 * (j / 8) as f32, 14.4, 14.4, [1.0; 4]);
    }
    let Some(m) = w.party.members().find(|m| m.id == id) else { return };
    let tint = if m.zone == w.pos_zone && m.away == 0 { 0x8080_8080 } else { 0x8040_4040 };
    if bracketed {
        gu.shape_text("< ", x0, y0 - 16.0, tint, true);
    }
    let end = gu.shape_text(&m.name, x0, y0 - 16.0, tint, false);
    if bracketed {
        gu.shape_text(" >", end, y0 - 16.0, tint, false);
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
