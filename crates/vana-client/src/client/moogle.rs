//! The moogle menus (s2c 0x02E, FUN_10097af0 -> FUN_101d6110): "menu    myroom" in one's own Mog House, "menu    mogext"
//! by a nomad moogle, their storage menus (FUN_10145e70) and Change Jobs. The Delivery Box, Gardening / Layout's own
//! modes and Remodel are not done yet.

use super::World;
use super::game_ui::GameUi;
use super::items;
use crate::fx::sched::Who;
use glam::Vec3;
use std::time::{Duration, Instant};

pub const MYROOM: &str = "menu    myroom";
pub const MOGEXT: &str = "menu    mogext";
const STORAGE: &str = "menu    storage";
const STORAGE2: &str = "menu    storage2";
const JOBCHANG: &str = "menu    jobchang";
const JOBCSELU: &str = "menu    jobcselu";
const MOGPOST: &str = "menu    mogpost";
/// FUN_101d32c0: the moogle's dance, scheduler 'main' of file 0x13ae.
const DANCE_FILE: u32 = 0x13ae;
const DANCE: u32 = u32::from_le_bytes(*b"main");

/// s2c 0x01B: the jobs we have (bit n = job n), each job's level, and whether a support job is unlocked.
#[derive(Clone, Copy, Debug)]
pub struct JobInfo {
    /// @8 / @0xb: the main and support job (s2c 0x044 is kept only for these, FUN_100f1050).
    pub main: u8,
    pub sub: u8,
    pub flags: u32,
    pub levels: [u8; 24],
    pub sub_ok: bool,
    /// Encumbered equipment slots (low 16 bits) and stat debilitations (player +0x40df0, s2c 0x01B @0x60).
    pub encumbrance: u32,
    /// @0x65 mentor, @0x66 mastery rank, @0x68 the jobs with a master level (bit n), @0x6c + n their levels.
    pub mentor: u8,
    pub mastery: u8,
    pub ml_jobs: u32,
    pub ml_levels: [u8; 24],
}

/// The Mog House / nomad object's request (+0x812..0x814): pending until c2s 0x100 goes; the last one is kept.
#[derive(Clone, Copy, Debug)]
pub struct JobReq {
    pub main: u8,
    pub sub: u8,
    pub pending: bool,
    pub moogle: Option<u32>,
}

/// jobchang's cursor and jobcselu over it (+0x14: 1 main / 2 support, its cursor child = job).
#[derive(Clone, Copy, Debug)]
pub struct Jobs {
    pub sel: u16,
    pub select: Option<(u8, u16)>,
}

/// The open moogle menu: myroom or mogext (`nomad`, with the spot it opened at), its cursor child, the storage menu
/// over it (cursor child, the children disabled at its open), and myroom +0x16 (an open request sent).
#[derive(Clone, Debug)]
pub struct Menu {
    pub nomad: bool,
    pub origin: Vec3,
    pub sel: u16,
    pub storage: Option<(u16, Vec<u16>)>,
    pub open_pending: bool,
    pub jobs: Option<Jobs>,
    /// "menu    mogpost" over it (FUN_10145c20): its cursor.
    pub post: Option<u16>,
}

impl Menu {
    fn name(&self) -> &'static str {
        if self.nomad { MOGEXT } else { MYROOM }
    }

    fn rows(&self) -> u16 {
        if self.nomad { 3 } else { 7 }
    }
}

/// The 0x20000 windows (window registry) that drop a 0x02E: the item / equipment windows, the map, the config windows,
/// the door menu, prompts, and ours that stand for them (the lists; inferred).
fn blocked(w: &World) -> bool {
    w.win.is_some() || w.map.is_some() || w.config.is_some() || w.settings.is_some() || w.mogdoor.is_some() || w.confirm.is_some() || w.magic.is_some() || w.ability.is_some()
}

/// Per frame: FUN_10097af0 (the pending 0x02E opens once no event runs) and FUN_10145680 (mogext and what it opened
/// close past 6 yalms from where it opened, x / z only).
pub fn update(w: &mut World) {
    if w.mog_pending && !super::event::active(w) {
        w.mog_pending = false;
        if !blocked(w) {
            open(w, w.myroom.is_none());
        }
    }
    if let Some(m) = &w.moogle
        && m.nomad
        && (w.pos.x - m.origin.x).powi(2) + (w.pos.z - m.origin.z).powi(2) > 36.0
    {
        if matches!(w.win, Some(items::Win::Items { store: Some(_), .. })) {
            w.win = None;
        }
        w.moogle = None;
    }
    // The per-frame send: c2s 0x100 {main, sub} once the moogle's dance is over (or there is no moogle).
    if let Some(r) = w.job_req.filter(|r| r.pending) {
        let busy = r.moogle.is_some_and(|id| w.entities.contains_key(&id) && w.effects.event_scheduler_running(DANCE_FILE, DANCE, Who::Actor(id), Who::Actor(w.my_id)));
        if !busy {
            let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x100, body: vec![r.main, r.sub, 0, 0] });
            w.job_req = Some(JobReq { pending: false, ..r });
        }
    }
}

/// myroom +0x18: within 3 s of a job change the moogle menus and the storage openers refuse (FUN_101039e0 <= it).
pub fn job_cooling(w: &World) -> bool {
    w.job_cooldown.is_some_and(|t| Instant::now() <= t)
}

/// The moogle actor (FUN_100a0080; retail by model 0x52 / 0x3d5 / 0x58a / 0x939, ours the nearest entity named
/// Moogle, inferred stand-in).
fn moogle_actor(w: &World) -> Option<u32> {
    w.entities
        .iter()
        .filter(|(_, e)| e.name.contains("Moogle"))
        .min_by(|a, b| (a.1.pos - w.pos).length().total_cmp(&(b.1.pos - w.pos).length()))
        .map(|(&id, _)| id)
}

/// jobcselu's disabled rows: the jobs whose 0x01B bit is clear.
fn jobs_off(w: &World) -> Vec<u16> {
    let flags = w.job_info.map_or(0, |j| j.flags);
    (1..=22u16).filter(|&n| flags & 1 << n == 0).collect()
}

/// (main, sub) as DAT_10485640 / DAT_10485642 hold them (s2c 0x061).
fn jobs_now(w: &World) -> (u8, u8) {
    w.stats.as_ref().map_or((0, 0), |s| (s.main.0, s.sub.0))
}

/// FUN_101d32c0 / FUN_101d52d0: refused (SE 0004) while one is pending or the same as the last; else the moogle
/// dances, and FUN_101a8030(0, 1, 0) turns the lockstyle off first (c2s 0x053 {0, Disable, 0}, 0x88 bytes). Nothing
/// is sent outside the Mog House or a MogZoneFlag zone. The monstrosity case (no 0x053, retried) is not done.
fn request(w: &mut World, main: u8, sub: u8) {
    if w.myroom.is_none() && !w.mog_zone {
        return;
    }
    if w.job_req.is_some_and(|r| r.pending || (r.main, r.sub) == (main, sub)) {
        w.sounds.push(crate::audio::Ui::Error as u32);
        return;
    }
    let moogle = moogle_actor(w);
    log::debug!("job change ({main}, {sub}), moogle {moogle:?}");
    if let Some(id) = moogle {
        w.effects.on_event_scheduler(DANCE_FILE, DANCE, Who::Actor(id), Who::Actor(w.my_id));
    }
    w.job_req = Some(JobReq { main, sub, pending: true, moogle });
    let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x053, body: vec![0; 0x84] });
}

/// jobchang (FUN_101459d0) and jobcselu (FUN_10145a90) keys.
fn jobs_key(w: &mut World, gu: &GameUi, j: Jobs, key: egui::Key) {
    use crate::audio::Ui;
    use egui::Key as K;
    let set = |w: &mut World, j: Option<Jobs>| {
        if let Some(m) = w.moogle.as_mut() {
            m.jobs = j;
        }
    };
    let Some((mode, sel)) = j.select else {
        match key {
            K::ArrowDown | K::ArrowUp => {
                set(w, Some(Jobs { sel: 3 - j.sel, ..j }));
                w.sounds.push(Ui::Cursor as u32);
            }
            // Escape refocuses myroom / mogext underneath.
            K::Escape => {
                set(w, None);
                w.sounds.push(Ui::Cancel as u32);
            }
            K::Enter if j.sel == 2 && !w.job_info.is_some_and(|i| i.sub_ok) => w.sounds.push(Ui::Error as u32),
            // The cursor starts on child 1 (inferred: the menu base's default).
            K::Enter => {
                set(w, Some(Jobs { select: Some((j.sel as u8, 1)), ..j }));
                w.sounds.push(Ui::Select as u32);
            }
            _ => {}
        }
        return;
    };
    let link = match key {
        K::ArrowUp => Some(2),
        K::ArrowDown => Some(3),
        K::ArrowRight => Some(4),
        K::ArrowLeft => Some(5),
        _ => None,
    };
    if let Some(k) = link {
        let to = gu.menu_links(JOBCSELU, sel).map_or(sel, |l| l[k] as u16);
        if to != sel && to != 0 {
            set(w, Some(Jobs { select: Some((mode, to)), ..j }));
            w.sounds.push(Ui::Cursor as u32);
        }
        return;
    }
    match key {
        K::Escape => {
            set(w, Some(Jobs { select: None, ..j }));
            w.sounds.push(Ui::Cancel as u32);
        }
        K::Enter if jobs_off(w).contains(&sel) => w.sounds.push(Ui::Error as u32),
        K::Enter => {
            w.sounds.push(Ui::Select as u32);
            let n = sel as u8;
            let (main, sub) = jobs_now(w);
            if n == main || n == sub {
                w.sounds.push(Ui::Error as u32);
                return;
            }
            request(w, if mode == 1 { n } else { 0 }, if mode == 1 { 0 } else { n });
            set(w, Some(Jobs { select: None, ..j }));
            w.job_cooldown = Some(Instant::now() + Duration::from_secs(3));
        }
        _ => {}
    }
}

/// FUN_101d6110(nomad): myroom in the Mog House (its open hook FUN_10145180 shuts it unless the house is ours), mogext
/// elsewhere, remembering our spot.
pub fn open(w: &mut World, nomad: bool) {
    if !nomad && w.mh_owner != 0 {
        return;
    }
    w.moogle = Some(Menu { nomad, origin: w.pos, sel: 1, storage: None, open_pending: false, jobs: None, post: None });
    w.menu = None;
    w.action_menu = None;
}

/// FUN_10145180 / FUN_10145240: myroom's disabled children. Row 4 (Gardening) with the moogle away; rows 5..7 only in
/// one's nation house or the Mog Garden: open to visitors -> only 6; else 5 / 7 with the moogle there and not in the
/// garden (7 also needs Safe 2); an open request pending -> none.
fn myroom_off(w: &World, m: &Menu) -> Vec<u16> {
    let present = super::myroom::in_city(w.zone);
    let room = w.myroom;
    let garden = room.is_some_and(|r| r.map == 0x2d4);
    let home = room.as_ref().is_some_and(super::myroom::has_floors);
    let mut off = Vec::new();
    if !present {
        off.push(4);
    }
    if home || garden {
        if w.mh_open {
            off.extend([5, 7]);
        } else if m.open_pending {
            off.extend([5, 6, 7]);
        } else {
            if !(present && !garden) {
                off.push(5);
            }
            if !(present && !garden && w.mh_floors) {
                off.push(7);
            }
        }
    } else {
        off.extend([5, 6, 7]);
    }
    off
}

/// FUN_10145ca0: the storage menu's children disabled at its open: the Satchel under 30, the Sack with no size, the Mog
/// Safe ("storage" only) with the moogle away, Safe 2 without the 2F flag, Wardrobes 2..8 with no size.
fn storage_off(w: &World, nomad: bool) -> Vec<u16> {
    let shift = if nomad { 1 } else { 0 };
    let n = |c: usize| w.inv.sizes[c];
    let mut off = Vec::new();
    if n(5) < 0x1e {
        off.push(5 - shift);
    }
    if n(6) == 0 {
        off.push(6 - shift);
    }
    if !nomad && !super::myroom::in_city(w.zone) {
        off.push(1);
    }
    if !w.mh_floors {
        off.push(2);
    }
    for c in 10..=16 {
        if n(c) == 0 {
            off.push((c as u16) - 1 - shift);
        }
    }
    off
}

/// A storage menu row's container (FUN_10145e70): Mog Safe, Safe 2, Storage ("storage" only), Locker, Satchel, Sack,
/// Case, Wardrobe, Wardrobe 2..8.
fn storage_container(row: u16, nomad: bool) -> Option<u8> {
    const ROWS: [u8; 15] = [1, 9, 2, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15, 16];
    let i = row.checked_sub(1)? as usize;
    if nomad {
        ROWS.iter().copied().filter(|&c| c != 2).nth(i)
    } else {
        ROWS.get(i).copied()
    }
}

/// The menu's keys (the menu base: links, SE 0001 / 0002 / 0004 / 0003). True when used.
pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) -> bool {
    use crate::audio::Ui;
    use egui::Key as K;
    let Some(m) = w.moogle.clone() else { return false };
    if let Some(j) = m.jobs {
        jobs_key(w, gu, j, key);
        return true;
    }
    // mogpost (FUN_10145bc0 / FUN_10145bf0): myroom runs /mailbox or /sendpost as menu lines, a nomad moogle sends
    // c2s 0x04D PostOpen / DeliOpen itself; it stays open, Escape closes it.
    if let Some(c) = m.post {
        match key {
            K::ArrowDown | K::ArrowUp => {
                if let Some(mm) = w.moogle.as_mut() {
                    mm.post = Some(3 - c);
                }
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Enter => {
                w.sounds.push(Ui::Select as u32);
                match (m.nomad, c) {
                    (false, 1) => super::textcmd::run(w, "/mailbox", super::textcmd::Mode::Internal),
                    (false, _) => super::textcmd::run(w, "/sendpost", super::textcmd::Mode::Internal),
                    (true, 1) => super::post::open_cmd(w, 0xe),
                    (true, _) => super::post::open_cmd(w, 0xd),
                }
            }
            K::Escape => {
                if let Some(mm) = w.moogle.as_mut() {
                    mm.post = None;
                }
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
        return true;
    }
    if let Some((sel, off)) = &m.storage {
        let n: u16 = if m.nomad { 14 } else { 15 };
        match key {
            K::ArrowDown | K::ArrowUp => {
                let s = if key == K::ArrowDown { sel % n + 1 } else if *sel <= 1 { n } else { sel - 1 };
                if let Some(mm) = w.moogle.as_mut() {
                    mm.storage = Some((s, off.clone()));
                }
                w.sounds.push(Ui::Cursor as u32);
            }
            K::Escape => {
                if let Some(mm) = w.moogle.as_mut() {
                    mm.storage = None;
                }
                w.sounds.push(Ui::Cancel as u32);
            }
            K::Enter if off.contains(sel) => w.sounds.push(Ui::Error as u32),
            K::Enter => {
                let Some(c) = storage_container(*sel, m.nomad) else { return true };
                w.sounds.push(Ui::Select as u32);
                // A 0 from FUN_101dae10 plays SE 0004; FUN_101daf70's result is not looked at.
                if matches!(c, 1 | 2 | 4 | 9) {
                    if !items::open_dae10(w, c, m.nomad && c != 2) {
                        w.sounds.push(Ui::Error as u32);
                    }
                } else {
                    items::open_daf70(w, c, c == 5, m.nomad);
                }
            }
            _ => {}
        }
        return true;
    }
    let off = if m.nomad { Vec::new() } else { myroom_off(w, &m) };
    let n = m.rows();
    match key {
        K::ArrowDown | K::ArrowUp => {
            if let Some(mm) = w.moogle.as_mut() {
                mm.sel = if key == K::ArrowDown { m.sel % n + 1 } else if m.sel <= 1 { n } else { m.sel - 1 };
            }
            w.sounds.push(Ui::Cursor as u32);
        }
        K::Escape => {
            w.moogle = None;
            w.sounds.push(Ui::Cancel as u32);
        }
        K::Enter if off.contains(&m.sel) => w.sounds.push(Ui::Error as u32),
        // FUN_10145460 / FUN_10145760: the synthesis gate, then the job-change cooldown (silent).
        K::Enter => {
            if !super::textcmd::status_gate(w, 0x04) || job_cooling(w) {
                w.sounds.push(Ui::Error as u32);
                return true;
            }
            w.sounds.push(Ui::Select as u32);
            match (m.nomad, m.sel) {
                (_, 1) => {
                    let off = storage_off(w, m.nomad);
                    if let Some(mm) = w.moogle.as_mut() {
                        mm.storage = Some((1, off));
                    }
                }
                // Gardening / Layout run their text commands and close myroom.
                (false, 4 | 5) => {
                    w.moogle = None;
                    super::textcmd::run(w, if m.sel == 4 { "/garden" } else { "/layout" }, super::textcmd::Mode::Internal);
                }
                // Open / Close Mog House: c2s 0x0CB {1 open | 2 close, 0, 0}; open waits for 0x067 @0x24.
                (false, 6) => {
                    let kind = if w.mh_open { 2u8 } else { 1 };
                    let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x0CB, body: vec![kind, 0, 0, 0] });
                    if kind == 1
                        && let Some(mm) = w.moogle.as_mut()
                    {
                        mm.open_pending = true;
                    }
                }
                // Change Jobs: myroom has the moogle say 0x32 "Chaaange...job!" when it is there.
                (_, 3) => {
                    if !m.nomad && super::myroom::in_city(w.zone) {
                        line(w, 0x32);
                    }
                    if let Some(mm) = w.moogle.as_mut() {
                        mm.jobs = Some(Jobs { sel: 1, select: None });
                    }
                }
                // Delivery Box: mogpost (FUN_10145ba0).
                (_, 2) => {
                    if let Some(mm) = w.moogle.as_mut() {
                        mm.post = Some(1);
                    }
                }
                (_, r) => log::info!("moogle menu row {r} (Remodel) is not implemented"),
            }
        }
        _ => {}
    }
    true
}

/// The windows: myroom / mogext at their layouts with the disabled children's dim sprites (myroom child 6 drawn as
/// windowps 606 "Open", 607 pending, 608 "Close"), the storage menu over them. Hidden while the storage view is up.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let mut rects = Vec::new();
    let Some(m) = &w.moogle else { return rects };
    if w.win.is_some() {
        return rects;
    }
    let mut off = if m.nomad { Vec::new() } else { myroom_off(w, m) };
    let child6 = (!m.nomad).then(|| if w.mh_open { 608 } else if m.open_pending { 607 } else { 606 });
    if child6.is_some() {
        off.retain(|&c| c != 6);
    }
    let mut hide = off.clone();
    if child6.is_some() {
        hide.push(6);
    }
    let free = !super::post::active(w);
    let focused = free && m.storage.is_none() && m.jobs.is_none() && m.post.is_none();
    let mut draw_one = |gu: &mut GameUi, name: &str, sel: u16, off: &[u16], hide: &[u16], focused: bool, extra: Option<(u16, usize)>| {
        if let Some(win) = gu.lobby_window_off(name, sw, sh, (0.0, 0.0), Some(sel), focused, hide) {
            let [x, y, ..] = win.frame;
            gu.disabled_children(name, x, y, off, focused.then_some(sel));
            if let Some(l) = gu.layout(name).cloned() {
                if let Some((child, sprite)) = extra
                    && let Some(e) = l.elements.iter().find(|e| e.id == child)
                {
                    gu.sprite("menu    windowps", sprite, x + e.x as f32, y + e.y as f32, 0x8080_8080);
                }
            }
            rects.push(win.frame);
        }
    };
    draw_one(gu, m.name(), m.sel, &off, &hide, focused, child6.map(|s| (6, s)));
    if let Some((sel, off)) = &m.storage {
        let name = if m.nomad { STORAGE2 } else { STORAGE };
        draw_one(gu, name, *sel, off, off, free, None);
    }
    if let Some(c) = m.post {
        draw_one(gu, MOGPOST, c, &[], &[], free, None);
    }
    if let Some(j) = m.jobs {
        let off: Vec<u16> = if w.job_info.is_some_and(|i| i.sub_ok) { Vec::new() } else { vec![2] };
        draw_one(gu, JOBCHANG, j.sel, &off, &off, free && j.select.is_none(), None);
        if let Some((_, sel)) = j.select {
            let off = jobs_off(w);
            draw_one(gu, JOBCSELU, sel, &off, &off, free, None);
            draw_levels(gu, w, &off, sw, sh);
        }
    }
    rects
}

/// FUN_101458f0: "L.%d" (group 10 0x5a) right-aligned at each unlocked row's (x1 + 0x1e, y0 + 2); gold for the main
/// job (its level from s2c 0x061), blue for the support job.
fn draw_levels(gu: &mut GameUi, w: &World, off: &[u16], sw: f32, sh: f32) {
    let Some([x, y, ..]) = gu.window_rect(JOBCSELU, sw, sh) else { return };
    let Some(l) = gu.layout(JOBCSELU).cloned() else { return };
    let fmt = gu.item_msg(10, 0x5a).unwrap_or_default();
    let (main, sub) = jobs_now(w);
    for e in l.elements.iter().skip(1).filter(|e| !off.contains(&e.id)) {
        let n = e.id as u8;
        let mut lv = w.job_info.map_or(0, |j| j.levels.get(n as usize).copied().unwrap_or(0));
        if n == main {
            lv = w.stats.as_ref().map_or(lv, |s| s.main.1);
        }
        let tint = if n == main { 0x80e0_9620 } else if n == sub { 0x8040_70a0 } else { 0x8080_8080 };
        let text = fmt.replace("%d", &lv.to_string());
        gu.shape_text(&text, x + e.x as f32 + e.w as f32 + 30.0, y + e.y as f32 + 2.0, tint, true);
    }
}

/// The bars: the focused window's child strings; myroom child 6 says Open (0x93 / 0x29a) or Close (0x94 / 0x29b).
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(m) = &w.moogle else { return };
    if w.win.is_some() {
        return;
    }
    let (name, child) = match (&m.storage, m.jobs) {
        _ if m.post.is_some() => (MOGPOST, m.post.unwrap_or(1)),
        (Some((sel, _)), _) => (if m.nomad { STORAGE2 } else { STORAGE }, *sel),
        (_, Some(Jobs { select: Some((_, sel)), .. })) => (JOBCSELU, sel),
        (_, Some(j)) => (JOBCHANG, j.sel),
        _ => (m.name(), m.sel),
    };
    let [help, title] = gu.focus_text(name, child);
    (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    if m.storage.is_none() && m.jobs.is_none() && m.post.is_none() && !m.nomad && m.sel == 6 {
        let (t, h) = if w.mh_open { (0x94, 0x29b) } else { (0x93, 0x29a) };
        bars.title[1] = gu.title_msg(t).unwrap_or_default();
        bars.help[1] = gu.menu_msg(h, "");
    }
}

/// FUN_100a1860(n): system message n (file 7031) spoken by the moogle (moogle_actor), "<name> : text".
pub fn line(w: &mut World, n: u16) {
    let Some(bt) = w.battle.clone() else { return };
    // Nothing without a moogle actor (all.c 120034).
    let Some(name) = moogle_actor(w).and_then(|id| w.entities.get(&id)).map(|e| e.name.clone()) else { return };
    let ctx = crate::dat::battle::Ctx { me: w.name.clone(), ..Default::default() };
    if let Some(l) = bt.system(n, &ctx)
        && let Some(text) = l.lines.into_iter().find(|l| !l.is_empty())
    {
        w.chat.push(format!("{name} : {text}"));
    }
}
