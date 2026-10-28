//! Mog House gardening (gardening_spec.md): /garden's Safe choice (myroom mode 2, bankmenu), the pot list (mode 3) with
//! the camera on the pot, the plnt windows by stage, the seed and crystal lists with their prompts, c2s 0x0FC..0x0FF;
//! in a room without furniture the moogle goes over to the Mog House instead (FUN_101d55d0 / FUN_101d5410).

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use crate::audio::Ui;
use crate::dat::furniture::FurnTable;
use crate::inventory::Slot;
use glam::Vec3;
use std::sync::Arc;

pub(super) const BANK: &str = "menu    bankmenu";
const INVENTOR: &str = "menu    inventor";
const WINDOWS: [&str; 6] = ["menu    plnt1", "menu    plnt2", "menu    plnt3", "menu    plnt4", "menu    plnt5", "menu    plnt6"];
/// Table 0x103854f4: the window (1..6) a stage opens; plnt5 is never opened.
const TABLE: [u8; 12] = [1, 6, 2, 2, 3, 2, 2, 3, 2, 2, 4, 6];
const POT: u16 = 12;
const SEED: u16 = 11;
const WHITE: u32 = 0x8080_8080;
const GREY: u32 = 0x8040_4040;
/// FUN_101d55d0's timer: 180 ticks.
const AWAY: f32 = 180.0;
/// File 0xbf8's 'main': the moogle going over (FUN_10062e20).
const AWAY_FILE: u32 = 0xbf8;
const AWAY_TAG: u32 = u32::from_le_bytes(*b"main");

/// The item list's sources: the pots (DAT_10664420), seeds (DAT_10664424), crystals (DAT_10664428).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Src {
    Pots,
    Seeds,
    Crystals,
}

/// What bankmenu picks a Safe for: the pots (LAB_101d3bf0), seeds (LAB_10206cd0), crystals (LAB_10206ea0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum For {
    Pots,
    Seeds,
    Crystals,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    Examine,
    Seeds,
    Crystal,
    Harvest,
    Dry,
    Throw,
    Exit,
}

/// The plnt windows' rows (3.1).
fn rows(win: u8) -> &'static [Row] {
    use Row::*;
    match win {
        1 => &[Examine, Seeds, Exit],
        2 => &[Examine, Dry, Throw, Exit],
        3 => &[Examine, Crystal, Dry, Throw, Exit],
        4 => &[Examine, Harvest, Dry, Throw, Exit],
        5 => &[Examine, Harvest, Exit],
        _ => &[Examine, Throw, Exit],
    }
}

/// The prompts (comyn, FUN_10210e30, default No): sow these seeds / use this crystal on the pot, gather, dry, throw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    Sow { c: u8, slot: u8, id: u16 },
    Feed { c: u8, slot: u8, id: u16 },
    Harvest,
    Dry,
    Throw,
}

pub struct Garden {
    table: Arc<FurnTable>,
    map: u16,
    floor: u8,
    /// FUN_101d5fe0: a room with furniture (the camera, the hidden player); else the moogle goes over.
    furnished: bool,
    /// myroom +0x1c: 2 the Safe's bankmenu, 3 gardening.
    mode: u8,
    /// FUN_101d3380(0) asked for: gardening ends at the next frame (the old mode decides the moogle's return).
    ending: bool,
    bank: Option<(u16, For)>,
    /// The pot source's container (+4) and the current pot's slot and item (+0xc).
    c: u8,
    cur: Option<(u8, u16)>,
    /// +8: the pot list up since its last Enter / Escape.
    list_up: bool,
    /// The item list: its source, scroll and cursor.
    list: Option<(Src, usize, usize)>,
    /// The saved rows: the pot list's (+0xa), the seed and crystal lists' (+8); their containers.
    saved: [usize; 3],
    src_c: [u8; 2],
    /// A plnt window (1..6) and its cursor row.
    win: Option<(u8, u16)>,
    /// FUN_101cda60's +0x2a: each pot's last stage.
    stages: std::collections::HashMap<(u8, u8), u8>,
}

fn item_type(w: &World, id: u16) -> u16 {
    w.item_info.as_ref().and_then(|i| i.item(id)).map_or(0, |it| it.item_type)
}

fn ext(w: &World, c: u8, slot: u8) -> [u8; 24] {
    w.inv.ext.get(&(c, slot)).copied().unwrap_or([0; 24])
}

/// FUN_102074e0: an installed flowerpot (type 12, exdata[1] bit 6).
fn is_pot(w: &World, c: u8, slot: u8, id: u16) -> bool {
    item_type(w, id) == POT && ext(w, c, slot)[1] & 0x40 != 0
}

/// A source's rows in slot order (container view 3), the locked ones grey (LockFlg not 0 / 5 / 0x1b): installed pots,
/// seeds (item type 11, FUN_10207130), crystals (0x1000..0x1007, FUN_10207250).
fn list_rows(w: &World, src: Src, c: u8) -> Vec<(Slot, u32)> {
    (1..=80u8)
        .filter_map(|s| w.inv.slots.get(&(c, s)).map(|&(id, count)| (s, id, count)))
        .filter(|&(s, id, _)| match src {
            Src::Pots => is_pot(w, c, s, id),
            Src::Seeds => item_type(w, id) == SEED,
            Src::Crystals => (0x1000..=0x1007).contains(&id),
        })
        .map(|(s, id, count)| {
            let lock = w.inv.locks.get(&(c, s)).copied().unwrap_or(0);
            (Slot { index: s, id, count, equipped: false, equipped_ex: false }, if matches!(lock, 0 | 5 | 0x1b) { WHITE } else { GREY })
        })
        .collect()
}

/// The window a pot's exdata opens (2.5): dried plnt6, else the stage's; none from 12 (gardening ends).
pub fn window_for(stage: u8, dried: bool) -> Option<u8> {
    if stage >= 12 {
        None
    } else if dried {
        Some(6)
    } else {
        Some(TABLE[stage as usize])
    }
}

/// FUN_10206830's moogle line (7031) by stage, `coin` the rand() & 1: dried 0x8b alone.
pub fn examine_line(stage: u8, dried: bool, coin: u16) -> Option<u16> {
    if dried {
        return Some(0x8b);
    }
    let base = match stage {
        0 => return Some(0x3e),
        1 => 0x3f,
        2 | 3 | 5 | 6 | 8 => 0x41,
        4 | 7 => 0x43,
        9 => 0x45,
        10 => 0x47,
        11 => 0x49,
        _ => return None,
    };
    Some(base + coin)
}

/// FUN_101d4500's points for a pot at `p` (y down): the look 0.4 above it, the eye 1.5 from it toward the room's axis at
/// that height; none on the axis.
pub fn camera_target(p: Vec3) -> Option<(Vec3, Vec3)> {
    let look = Vec3::new(p.x, p.y - 0.4, p.z);
    let d = Vec3::new(-p.x, 0.0, -p.z).try_normalize()?;
    Some((Vec3::new(p.x + 1.5 * d.x, p.y - 0.4, p.z + 1.5 * d.z), look))
}

/// FUN_101f7080(row, 0): the saved row comes back as the top row (the last page clamped).
pub fn restore_top(row: usize, n: usize) -> (usize, usize) {
    let top = row.min(n.saturating_sub(crate::inventory::VISIBLE_ROWS));
    (top, row.saturating_sub(top).min(n.saturating_sub(top + 1)))
}

/// c2s 0x0FC (FUN_100f0da0): pot item, added item, pot slot, added slot, pot container, added container.
pub fn add_body(pot: (u8, u8, u16), add: (u8, u8, u16)) -> Vec<u8> {
    let mut b = Vec::with_capacity(8);
    b.extend_from_slice(&pot.2.to_le_bytes());
    b.extend_from_slice(&add.2.to_le_bytes());
    b.extend_from_slice(&[pot.1, add.1, pot.0, add.0]);
    b
}

/// c2s 0x0FD (Examine) / 0x0FF (Dry): item, slot, container.
pub fn pot_body(pot: (u8, u8, u16)) -> Vec<u8> {
    let [a, b] = pot.2.to_le_bytes();
    vec![a, b, pot.1, pot.0]
}

/// c2s 0x0FE: item, slot, container, CancellFlg (0 Harvest, 1 Empty Pot), padding.
pub fn crop_body(pot: (u8, u8, u16), cancel: bool) -> Vec<u8> {
    let mut b = pot_body(pot);
    b.extend_from_slice(&[cancel as u8, 0, 0, 0]);
    b
}

/// The room's pots and their stages now.
fn pot_stages(w: &World) -> std::collections::HashMap<(u8, u8), u8> {
    w.inv.slots.iter().filter(|&(&(c, s), &(id, _))| matches!(c, 1 | 9) && is_pot(w, c, s, id)).map(|(&k, _)| (k, ext(w, k.0, k.1)[0])).collect()
}

/// FUN_101d3380(2, -1, 1) (/garden, after its gate): bankmenu for the Safe.
pub fn start(w: &mut World) {
    let Some(r) = w.myroom else { return };
    let Some(index) = w.index.clone() else { return };
    let table = match FurnTable::load(&index) {
        Ok(t) => Arc::new(t),
        Err(e) => {
            log::warn!("garden: furniture table: {e:#}");
            return;
        }
    };
    let stages = pot_stages(w);
    w.garden = Some(Garden {
        table,
        map: r.map,
        floor: r.floor,
        furnished: super::myroom::has_floors(&r),
        mode: 2,
        ending: false,
        bank: Some((1, For::Pots)),
        c: 1,
        cur: None,
        list_up: false,
        list: None,
        saved: [0; 3],
        src_c: [1, 1],
        win: None,
        stages,
    });
    log::info!("garden: started, floor {}, furnished {}", r.floor, super::myroom::has_floors(&r));
}

/// A gardening window has the keys: bankmenu, the list or a plnt window.
pub fn has_window(w: &World) -> bool {
    w.garden.as_ref().is_some_and(|g| g.bank.is_some() || g.list.is_some() || g.win.is_some())
}

/// Mode 3 in a furnished room: the player is not drawn (FUN_101d4d90: +0xb2 bit 4).
pub fn hides_player(w: &World) -> bool {
    w.garden.as_ref().is_some_and(|g| g.mode == 3 && g.furnished)
}

/// The control lock (DAT_10482f18) while the moogle goes over or comes back.
pub fn locked(w: &World) -> bool {
    w.garden_away.is_some_and(|(_, t)| t >= 0.0)
}

/// Mode 3's context 0x42 (FUN_101236e0) in a furnished room or upstairs: no walking, no camera keys.
pub fn context(w: &World) -> Option<u16> {
    w.garden.as_ref().filter(|g| g.mode == 3 && (g.furnished || g.floor != 0)).map(|_| 0x42)
}

/// DAT_1037eb94[3]: the focal while gardening in a furnished room.
pub fn focal(w: &World) -> Option<f32> {
    hides_player(w).then_some(350.0)
}

/// DAT_1037eba8[floor]: the room's middle.
fn park(floor: u8) -> Vec3 {
    if floor == 0 { Vec3::ZERO } else { Vec3::new(0.0, 0.0, -9.0) }
}

/// FUN_101d55d0: the moogle going over (1: 0x50 on 1F, the control lock, file 0xbf8's 'main' on it) or coming back
/// (2: shown again, fading in).
fn moogle_away(w: &mut World, going: bool, floor: u8) {
    w.garden_away = Some((if going { 1 } else { 2 }, AWAY));
    if going {
        if floor == 0 {
            super::moogle::line(w, 0x50);
        }
        if let Some(id) = super::moogle::moogle_actor(w) {
            w.effects.on_event_scheduler(AWAY_FILE, AWAY_TAG, crate::fx::sched::Who::Actor(id), crate::fx::sched::Who::Actor(id));
        }
    } else {
        w.moogle_hidden = false;
    }
}

/// FUN_101d3380(0, -1, c): gardening ends. Furnished: the player back at the room's middle with the normal camera, the
/// moogle's menu again; else the moogle comes back (also when it never left, as read).
fn leave(w: &mut World) {
    let Some(g) = w.garden.take() else { return };
    if g.furnished {
        w.pos = park(g.floor);
        let yaw = w.yaw;
        super::camera::face(w, yaw);
        if w.moogle.is_none() && !super::event::active(w) {
            super::moogle::open(w, false);
        }
    } else if g.mode != 0 {
        moogle_away(w, false, g.floor);
    }
    log::info!("garden: left");
}

/// Mode 3 (FUN_101d3380 case 3): no pot in the Safe -> 0x3b and back; furnished or upstairs the pot list; else the
/// moogle goes over and the list opens on its first frame.
fn enter(w: &mut World, g: &mut Garden, c: u8) {
    g.c = c;
    if list_rows(w, Src::Pots, c).is_empty() {
        super::moogle::line(w, 0x3b);
        g.ending = true;
        return;
    }
    (w.menu, w.action_menu, w.held_menu, w.moogle, w.target, w.locked) = (None, None, None, None, None, false);
    g.mode = 3;
    if g.furnished {
        w.pos = park(g.floor);
    }
    if g.furnished || g.floor != 0 {
        open_list(w, g, Src::Pots);
    } else {
        moogle_away(w, true, g.floor);
    }
}

/// FUN_101f23a0 with the source's saved row on top; the pot list's open hook clears the current pot and, furnished
/// on 1F, has the moogle say 0x3d (every open).
fn open_list(w: &mut World, g: &mut Garden, src: Src) {
    let (k, c) = match src {
        Src::Pots => (0, g.c),
        Src::Seeds => (1, g.src_c[0]),
        Src::Crystals => (2, g.src_c[1]),
    };
    let (top, cur) = restore_top(g.saved[k], list_rows(w, src, c).len());
    g.list = Some((src, top, cur));
    if src == Src::Pots {
        g.cur = None;
        if g.furnished && g.floor == 0 {
            super::moogle::line(w, 0x3d);
        }
    }
}

/// The list closes, its row saved.
fn close_list(g: &mut Garden) {
    if let Some((src, top, cur)) = g.list.take() {
        let k = match src {
            Src::Pots => 0,
            Src::Seeds => 1,
            Src::Crystals => 2,
        };
        g.saved[k] = top + cur;
    }
    g.list_up = false;
}

/// "→ list": the pot list back on its saved row, the plnt window closed.
fn back_to_pots(w: &mut World, g: &mut Garden) {
    g.win = None;
    close_list(g);
    open_list(w, g, Src::Pots);
}

/// The current pot (container, slot, item).
fn pot(g: &Garden) -> Option<(u8, u8, u16)> {
    g.cur.map(|(s, id)| (g.c, s, id))
}

fn send(w: &World, id: u16, body: Vec<u8>) {
    log::info!("garden: c2s {id:#05x} {body:02x?}");
    let _ = w.net.commands.send(NetCommand::Packet { id, body });
}

/// Every frame: an event ends gardening (FUN_101d4a20); the moogle's timers; the pot under the list's cursor; a stage
/// change of the current pot closes its windows and brings the list back (FUN_101cda60).
pub fn update(w: &mut World, dt: f32) {
    away_step(w, dt * 60.0);
    if w.garden.is_none() {
        return;
    }
    if super::event::active(w) || w.myroom.is_none() {
        leave(w);
        return;
    }
    let Some(mut g) = w.garden.take() else { return };
    if g.ending {
        w.garden = Some(g);
        leave(w);
        return;
    }
    if let Some((Src::Pots, top, cur)) = g.list {
        let rows = list_rows(w, Src::Pots, g.c);
        g.cur = rows.get(top + cur).map(|(s, _)| (s.index, s.id));
        g.list_up = true;
    }
    for (k, stage) in pot_stages(w) {
        if g.stages.insert(k, stage).is_some_and(|old| old != stage) && stage != 0 && g.mode == 3 && pot(&g).is_some_and(|p| (p.0, p.1) == k) {
            g.win = None;
            if matches!(w.confirm, Some((super::Confirm::Garden(_), _))) {
                w.confirm = None;
            }
            if !g.list_up {
                open_list(w, &mut g, Src::Pots);
            }
        }
    }
    w.garden = Some(g);
}

/// FUN_101d5410's states: going, the list opens on the first frame, then the moogle is hidden and says 0x51; coming
/// back it fades in ((180 - t) x 0x80 / 180) and says 0x52.
fn away_step(w: &mut World, ticks: f32) {
    let Some((state, t)) = w.garden_away else { return };
    let floor = w.garden.as_ref().map_or(0, |g| g.floor);
    if state == 1 && t == AWAY
        && let Some(mut g) = w.garden.take()
    {
        open_list(w, &mut g, Src::Pots);
        w.garden = Some(g);
    }
    let t = t - ticks;
    let moogle = super::moogle::moogle_actor(w);
    if state == 2
        && let Some(a) = moogle.and_then(|id| w.actors.get_mut(&id))
    {
        a.alpha = (((AWAY - t.max(0.0)) * 128.0) / AWAY).floor().min(128.0);
    }
    if t < 0.0 {
        w.garden_away = None;
        if state == 1 {
            w.moogle_hidden = true;
            if floor == 0 {
                super::moogle::line(w, 0x51);
            }
        } else if floor == 0 {
            super::moogle::line(w, 0x52);
        }
    } else {
        w.garden_away = Some((state, t));
    }
}

/// The camera in mode 3 of a furnished room (FUN_101d4500): on the current pot, eased 0.3 a retail frame; still without
/// one. False otherwise.
pub fn camera(w: &mut World, ticks: f32) -> bool {
    let Some(g) = w.garden.as_ref().filter(|g| g.mode == 3 && g.furnished) else { return false };
    let target = pot(g).filter(|_| g.floor == 0).and_then(|(c, s, id)| {
        let e = ext(w, c, s);
        let f = g.table.entry(id)?;
        let (p, _) = super::furniture::place(&f, g.map, g.floor, e[6] as i8, e[7] as i8, e[8] as i8, e[9])?;
        camera_target(p)
    });
    if let Some((eye, look)) = target {
        let k = super::layout::ease(ticks);
        w.cam.look += (look - w.cam.look) * k;
        w.cam.eye += (eye - w.cam.eye) * k;
    }
    true
}

pub fn key(w: &mut World, key: egui::Key) {
    let Some(mut g) = w.garden.take() else { return };
    if w.garden_away.is_some_and(|(s, _)| s == 1) {
        w.garden = Some(g);
        return;
    }
    if let Some((row, f)) = g.bank {
        bank_key(w, &mut g, row, f, key);
    } else if g.list.is_some() {
        list_key(w, &mut g, key);
    } else if let Some((win, row)) = g.win {
        win_key(w, &mut g, win, row, key);
    }
    w.garden = Some(g);
}

/// bankmenu's dim row: Safe 2 until it is unlocked.
fn bank_dim(w: &World) -> Vec<u16> {
    if w.mh_floors { Vec::new() } else { vec![2] }
}

/// bankmenu (FUN_101d6d10): Up / Down, Enter picks Mog Safe (1) or Safe 2 (9), Escape cancels.
fn bank_key(w: &mut World, g: &mut Garden, row: u16, f: For, key: egui::Key) {
    use egui::Key as K;
    match key {
        K::ArrowUp | K::ArrowDown => {
            g.bank = Some((3 - row, f));
            w.sounds.push(Ui::Cursor as u32);
        }
        K::Escape => {
            g.bank = None;
            w.sounds.push(Ui::Cancel as u32);
            // LAB_101d3bf0: cancelled, gardening ends (2 -> 0: the player to the room's middle, as read).
            if f == For::Pots {
                g.ending = true;
            }
        }
        K::Enter if bank_dim(w).contains(&row) => w.sounds.push(Ui::Error as u32),
        K::Enter => {
            w.sounds.push(Ui::Select as u32);
            g.bank = None;
            let c = if row == 1 { 1 } else { 9 };
            match f {
                For::Pots => enter(w, g, c),
                // LAB_10206cd0 / LAB_10206ea0: the seed / crystal list on that Safe.
                For::Seeds => {
                    g.src_c[0] = c;
                    open_list(w, g, Src::Seeds);
                    super::moogle::line(w, 0x4b);
                }
                For::Crystals => {
                    g.src_c[1] = c;
                    open_list(w, g, Src::Crystals);
                    super::moogle::line(w, 0x4c);
                }
            }
        }
        _ => {}
    }
}

/// The item list (FUN_10207510 and the seed / crystal sources' handlers).
fn list_key(w: &mut World, g: &mut Garden, key: egui::Key) {
    use egui::Key as K;
    let Some((src, mut top, mut cur)) = g.list else { return };
    let c = match src {
        Src::Pots => g.c,
        Src::Seeds => g.src_c[0],
        Src::Crystals => g.src_c[1],
    };
    let rows = list_rows(w, src, c);
    match key {
        K::Escape => match src {
            // 2.5: unfurnished or upstairs the moogle comes back; gardening ends.
            Src::Pots => {
                w.sounds.push(Ui::Cancel as u32);
                close_list(g);
                g.win = None;
                if !g.furnished || g.floor != 0 {
                    moogle_away(w, false, g.floor);
                }
                g.ending = true;
            }
            // The seed list: back to the pots (the plnt window stays); the crystal list closes the plnt windows too.
            Src::Seeds | Src::Crystals => {
                w.sounds.push(Ui::Cancel as u32);
                if src == Src::Crystals {
                    g.win = None;
                }
                close_list(g);
                open_list(w, g, Src::Pots);
            }
        },
        K::Enter => {
            let Some((s, tint)) = rows.get(top + cur).cloned() else { return };
            if tint == GREY {
                w.sounds.push(Ui::Error as u32);
                return;
            }
            match src {
                Src::Pots => {
                    let e = ext(w, c, s.index);
                    close_list(g);
                    g.cur = Some((s.index, s.id));
                    w.sounds.push(Ui::Select as u32);
                    match window_for(e[0], e[1] & 0x80 != 0) {
                        Some(win) => g.win = Some((win, 1)),
                        None => g.ending = true,
                    }
                }
                Src::Seeds => {
                    close_list(g);
                    w.confirm = Some((super::Confirm::Garden(Ask::Sow { c, slot: s.index, id: s.id }), false));
                    super::moogle::line(w, 0x53);
                    w.sounds.push(Ui::Select as u32);
                }
                // FUN_10207270: no sound (as read).
                Src::Crystals => {
                    close_list(g);
                    w.confirm = Some((super::Confirm::Garden(Ask::Feed { c, slot: s.index, id: s.id }), false));
                    super::moogle::line(w, 0x54);
                }
            }
        }
        _ => {
            if super::items::list_keys(&mut top, &mut cur, rows.len(), key) {
                w.sounds.push(Ui::Cursor as u32);
                g.list = Some((src, top, cur));
            }
        }
    }
}

/// A plnt window (the generic menu with its select handler, 3.2): rows wrap; Escape is the pot list again.
fn win_key(w: &mut World, g: &mut Garden, win: u8, row: u16, key: egui::Key) {
    use egui::Key as K;
    let n = rows(win).len() as u16;
    match key {
        K::ArrowUp | K::ArrowLeft => {
            g.win = Some((win, if row <= 1 { n } else { row - 1 }));
            w.sounds.push(Ui::Cursor as u32);
        }
        K::ArrowDown | K::ArrowRight => {
            g.win = Some((win, if row >= n { 1 } else { row + 1 }));
            w.sounds.push(Ui::Cursor as u32);
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            back_to_pots(w, g);
        }
        K::Enter => {
            w.sounds.push(Ui::Select as u32);
            let Some(p) = pot(g) else { return };
            match rows(win)[row as usize - 1] {
                Row::Examine => {
                    send(w, 0x0FD, pot_body(p));
                    let e = ext(w, p.0, p.1);
                    let coin = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos()) & 1) as u16;
                    if let Some(n) = examine_line(e[0], e[1] & 0x80 != 0, coin) {
                        super::moogle::line(w, n);
                    }
                }
                Row::Seeds => g.bank = Some((1, For::Seeds)),
                Row::Crystal => g.bank = Some((1, For::Crystals)),
                Row::Exit if win == 5 => g.win = None,
                Row::Exit => back_to_pots(w, g),
                r @ (Row::Harvest | Row::Dry | Row::Throw) => {
                    g.win = None;
                    let (ask, lines) = match r {
                        Row::Harvest => (Ask::Harvest, [0x4e, 0x55]),
                        Row::Dry => (Ask::Dry, [0x4d, 0x56]),
                        _ => (Ask::Throw, [0x4f, 0x57]),
                    };
                    w.confirm = Some((super::Confirm::Garden(ask), false));
                    for n in lines {
                        super::moogle::line(w, n);
                    }
                }
            }
        }
        _ => {}
    }
}

/// A prompt answered (Yes `yes`, else No or Escape): Yes sends its packet for the pot as it is now; either way the pot
/// list comes back on its saved row.
pub fn confirmed(w: &mut World, a: Ask, yes: bool) {
    let Some(mut g) = w.garden.take() else { return };
    if yes && let Some(p) = pot(&g) {
        match a {
            Ask::Sow { c, slot, id } | Ask::Feed { c, slot, id } => send(w, 0x0FC, add_body(p, (c, slot, id))),
            Ask::Harvest => send(w, 0x0FE, crop_body(p, false)),
            Ask::Dry => send(w, 0x0FF, pot_body(p)),
            Ask::Throw => send(w, 0x0FE, crop_body(p, true)),
        }
    }
    if g.mode == 3 {
        back_to_pots(w, &mut g);
    }
    w.garden = Some(g);
}

/// The prompt's text (group 0xc 0x30..0x34).
pub fn ask_text(gu: &GameUi, a: Ask) -> String {
    match a {
        Ask::Sow { .. } => gu.lobby_msg(0x0c, 0x30, "Sow seeds?"),
        Ask::Dry => gu.lobby_msg(0x0c, 0x31, "Dry out this plant?"),
        Ask::Feed { .. } => gu.lobby_msg(0x0c, 0x32, "Nourish the plant with this crystal?"),
        Ask::Harvest => gu.lobby_msg(0x0c, 0x33, "Gather current plant yields?"),
        Ask::Throw => gu.lobby_msg(0x0c, 0x34, "Throw this away?"),
    }
}

/// The list's cursor item, for the item info window (mode 0, stacked on the log).
pub fn info_source(w: &World) -> Option<(u16, Option<[u8; 24]>)> {
    let g = w.garden.as_ref()?;
    let (src, top, cur) = g.list?;
    let c = match src {
        Src::Pots => g.c,
        Src::Seeds => g.src_c[0],
        Src::Crystals => g.src_c[1],
    };
    let (s, _) = list_rows(w, src, c).get(top + cur).cloned()?;
    Some((s.id, w.inv.ext.get(&(c, s.index)).copied()))
}

/// Title and help: a plnt window's "Gardening" (0xd 0x59) and its row's help, bankmenu's row's, the list's "Select an
/// item." (0xe 3).
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(g) = &w.garden else { return };
    if let Some((row, _)) = g.bank {
        let [help, title] = gu.focus_text(BANK, row);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    } else if g.list.is_some() {
        bars.help[1] = gu.menu_msg(3, "");
        bars.title[1] = gu.title_msg(2).unwrap_or_default();
    } else if let Some((win, row)) = g.win {
        let [help, title] = gu.focus_text(WINDOWS[win as usize - 1], row);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    }
}

/// The plnt window (stacked on the log), bankmenu over it, the item list.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut out = Vec::new();
    let Some(g) = &w.garden else { return out };
    if w.modern {
        return out;
    }
    if let Some((win, row)) = g.win {
        let name = WINDOWS[win as usize - 1];
        let focus = w.confirm.is_none() && g.bank.is_none() && g.list.is_none();
        if let Some(lw) = gu.lobby_window_off(name, sw, sh, (0.0, 0.0), Some(row), focus, &[]) {
            super::winhits::record(w, name, &lw);
            out.push(lw.frame);
        }
    }
    if let Some((row, _)) = g.bank {
        let off = bank_dim(w);
        if let Some(lw) = gu.lobby_window_off(BANK, sw, sh, (0.0, 0.0), Some(row), w.confirm.is_none(), &off) {
            super::winhits::record(w, BANK, &lw);
            let [x, y, ..] = lw.frame;
            gu.disabled_children(BANK, x, y, &off, Some(row));
            out.push(lw.frame);
        }
    }
    if let Some((src, top, cur)) = g.list
        && let Some(info) = w.item_info.clone()
    {
        let c = match src {
            Src::Pots => g.c,
            Src::Seeds => g.src_c[0],
            Src::Crystals => g.src_c[1],
        };
        let rows = list_rows(w, src, c);
        let slots: Vec<_> = rows.iter().map(|r| r.0.clone()).collect();
        let mut list = crate::inventory::ItemList { rows: crate::inventory::build_rows(&slots, info.as_ref(), false), scroll: top, cursor: cur };
        for (r, (_, tint)) in list.rows.iter_mut().zip(&rows) {
            r.tint = *tint;
        }
        super::items::upload_icons(gu, &info, list.visible().iter().map(|r| r.id));
        let open = gu.gate_layout(INVENTOR, sw, sh, None);
        let frame = gu.cursor_frame();
        let (b, ui, style, font) = gu.parts();
        if let Some(m) = ui.menu(INVENTOR).filter(|_| open) {
            let _ = crate::render::invwin::push_inventory(b, ui, style, font, GameUi::ATLAS, m, &list, sw, sh, if w.confirm.is_none() { frame } else { usize::MAX });
            out.extend(gu.window_rect(INVENTOR, sw, sh));
        }
    }
    out
}

/// The mouse: the plnt window and bankmenu as the generic window; the list's rows as the item list (hover moves the
/// row, a click on it is Enter, a right click Escape).
pub fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    use super::winhits::generic;
    if w.modern {
        return;
    }
    let Some(g) = &w.garden else { return };
    let keys = |w: &mut World, _: &GameUi, k: egui::Key| key(w, k);
    if let Some((row, f)) = g.bank {
        generic(w, gu, BANK, row, 1..=2, p, moved, clicked, right, |w, id| {
            if let Some(g) = w.garden.as_mut() {
                g.bank = Some((id, f));
            }
        }, keys);
    } else if let Some((src, top, cur)) = g.list {
        let c = match src {
            Src::Pots => g.c,
            Src::Seeds => g.src_c[0],
            Src::Crystals => g.src_c[1],
        };
        let n = list_rows(w, src, c).len();
        let Some(r) = gu.window_rect(INVENTOR, sw, sh) else { return };
        let row = ((p.1 - r[1] - 4.0) / crate::inventory::ROW_PITCH).floor();
        let on = p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3];
        if right && on {
            key(w, egui::Key::Escape);
            return;
        }
        if !on || row < 0.0 || row as usize >= crate::inventory::VISIBLE_ROWS || top + row as usize >= n {
            return;
        }
        gu.hover.set(true);
        let row = row as usize;
        if row != cur && (moved || clicked) {
            if let Some(g) = w.garden.as_mut() {
                g.list = Some((src, top, row));
            }
        } else if clicked {
            key(w, egui::Key::Enter);
        }
    } else if let Some((win, row)) = g.win {
        let n = rows(win).len() as u16;
        generic(w, gu, WINDOWS[win as usize - 1], row, 1..=n, p, moved, clicked, right, |w, id| {
            if let Some(g) = w.garden.as_mut() {
                g.win = Some((win, id));
            }
        }, keys);
    }
}

/// Gardening's windows as the modern layer draws them: the plnt window (name, cursor), bankmenu, the list.
pub(super) struct Look {
    pub win: Option<(&'static str, u16)>,
    pub bank: Option<(u16, Vec<u16>)>,
    pub list: Option<(Vec<Slot>, Vec<u32>, usize)>,
}

pub(super) fn look(w: &World) -> Option<Look> {
    let g = w.garden.as_ref()?;
    let list = g.list.map(|(src, top, cur)| {
        let c = match src {
            Src::Pots => g.c,
            Src::Seeds => g.src_c[0],
            Src::Crystals => g.src_c[1],
        };
        let rows = list_rows(w, src, c);
        (rows.iter().map(|r| r.0.clone()).collect(), rows.iter().map(|r| r.1).collect(), top + cur)
    });
    Some(Look { win: g.win.map(|(win, row)| (WINDOWS[win as usize - 1], row)), bank: g.bank.map(|(r, _)| (r, bank_dim(w))), list })
}

pub(super) fn set_win(w: &mut World, k: u16) -> bool {
    let Some(g) = w.garden.as_mut().filter(|g| g.bank.is_none() && g.list.is_none()) else { return false };
    let Some((_, row)) = g.win.as_mut().filter(|(win, row)| *row != k && (1..=rows(*win).len() as u16).contains(&k)) else { return false };
    *row = k;
    true
}

pub(super) fn set_bank(w: &mut World, k: u16) -> bool {
    let Some((r, _)) = w.garden.as_mut().and_then(|g| g.bank.as_mut()).filter(|(r, _)| *r != k && (1..=2).contains(&k)) else { return false };
    *r = k;
    true
}

pub(super) fn set_row(w: &mut World, k: usize) -> bool {
    let Some(g) = w.garden.as_ref() else { return false };
    let Some((src, top, cur)) = g.list else { return false };
    let c = match src {
        Src::Pots => g.c,
        Src::Seeds => g.src_c[0],
        Src::Crystals => g.src_c[1],
    };
    if top + cur == k || k >= list_rows(w, src, c).len() {
        return false;
    }
    let t = top.min(k).max((k + 1).saturating_sub(crate::inventory::VISIBLE_ROWS));
    if let Some(g) = w.garden.as_mut() {
        g.list = Some((src, t, k - t));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_lines_and_bodies() {
        assert_eq!((0..12).map(|s| window_for(s, false).unwrap()).collect::<Vec<_>>(), [1, 6, 2, 2, 3, 2, 2, 3, 2, 2, 4, 6]);
        assert_eq!((window_for(4, true), window_for(12, false)), (Some(6), None));
        assert_eq!((examine_line(0, false, 1), examine_line(10, false, 1), examine_line(4, true, 0)), (Some(0x3e), Some(0x48), Some(0x8b)));
        // Porcelain Flowerpot 216 slot 5 of the Safe, Herb Seeds 572 slot 7.
        assert_eq!(add_body((1, 5, 216), (1, 7, 572)), [0xd8, 0x00, 0x3c, 0x02, 0x05, 0x07, 0x01, 0x01]);
        assert_eq!(pot_body((1, 5, 216)), [0xd8, 0x00, 0x05, 0x01]);
        assert_eq!(crop_body((1, 5, 216), true), [0xd8, 0x00, 0x05, 0x01, 0x01, 0, 0, 0]);
        let (eye, look) = camera_target(Vec3::new(4.5, -0.5, -3.9)).unwrap();
        assert!((look - Vec3::new(4.5, -0.9, -3.9)).length() < 1e-5);
        assert!((eye - Vec3::new(3.36647, -0.9, -2.9176)).length() < 1e-4, "{eye}");
        assert_eq!(restore_top(3, 40), (3, 0));
        assert_eq!(restore_top(38, 40), (40 - crate::inventory::VISIBLE_ROWS, 38 - (40 - crate::inventory::VISIBLE_ROWS)));
    }
}
