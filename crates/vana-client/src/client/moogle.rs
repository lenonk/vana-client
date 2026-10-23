//! The moogle menus (s2c 0x02E, FUN_10097af0 -> FUN_101d6110): "menu    myroom" in one's own Mog House, "menu    mogext"
//! by a nomad moogle, and their storage menus (FUN_10145e70). Change Jobs, the Delivery Box, Gardening / Layout's own
//! modes and Remodel are not done yet.

use super::World;
use super::game_ui::GameUi;
use super::items;
use glam::Vec3;

pub const MYROOM: &str = "menu    myroom";
pub const MOGEXT: &str = "menu    mogext";
const STORAGE: &str = "menu    storage";
const STORAGE2: &str = "menu    storage2";

/// The open moogle menu: myroom or mogext (`nomad`, with the spot it opened at), its cursor child, the storage menu
/// over it (cursor child, the children disabled at its open), and myroom +0x16 (an open request sent).
#[derive(Clone, Debug)]
pub struct Menu {
    pub nomad: bool,
    pub origin: Vec3,
    pub sel: u16,
    pub storage: Option<(u16, Vec<u16>)>,
    pub open_pending: bool,
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
}

/// FUN_101d6110(nomad): myroom in the Mog House (its open hook FUN_10145180 shuts it unless the house is ours), mogext
/// elsewhere, remembering our spot.
pub fn open(w: &mut World, nomad: bool) {
    if !nomad && w.mh_owner != 0 {
        return;
    }
    w.moogle = Some(Menu { nomad, origin: w.pos, sel: 1, storage: None, open_pending: false });
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
pub fn key(w: &mut World, key: egui::Key) -> bool {
    use crate::audio::Ui;
    use egui::Key as K;
    let Some(m) = w.moogle.clone() else { return false };
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
        // FUN_10145460 / FUN_10145760: the synthesis gate first (the job-change cooldown is not tracked).
        K::Enter => {
            if !super::textcmd::status_gate(w, 0x04) {
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
                (_, r) => log::info!("moogle menu row {r} (Delivery Box / Change Jobs / Remodel) is not implemented"),
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
    let focused = m.storage.is_none();
    let mut draw_one = |gu: &mut GameUi, name: &str, sel: u16, off: &[u16], hide: &[u16], focused: bool, extra: Option<(u16, usize)>| {
        if let Some(win) = gu.lobby_window_off(name, sw, sh, (0.0, 0.0), Some(sel), focused, hide) {
            let [x, y, ..] = win.frame;
            if let Some(l) = gu.layout(name).cloned() {
                for e in l.elements.iter().filter(|e| off.contains(&e.id)) {
                    if let Some(r) = e.refs.iter().find(|r| r.kind == 4) {
                        gu.sprite(&r.lib, r.index as usize, x + e.x as f32, y + e.y as f32, 0x8080_8080);
                    }
                }
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
        draw_one(gu, name, *sel, off, off, true, None);
    }
    rects
}

/// The bars: the focused window's child strings; myroom child 6 says Open (0x93 / 0x29a) or Close (0x94 / 0x29b).
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(m) = &w.moogle else { return };
    if w.win.is_some() {
        return;
    }
    let (name, child) = match &m.storage {
        Some((sel, _)) => (if m.nomad { STORAGE2 } else { STORAGE }, *sel),
        None => (m.name(), m.sel),
    };
    let [help, title] = gu.focus_text(name, child);
    (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    if m.storage.is_none() && !m.nomad && m.sel == 6 {
        let (t, h) = if w.mh_open { (0x94, 0x29b) } else { (0x93, 0x29a) };
        bars.title[1] = gu.title_msg(t).unwrap_or_default();
        bars.help[1] = gu.menu_msg(h, "");
    }
}

/// FUN_100a1860(n): system message n (file 7031) spoken by the moogle, "<name> : text" (the moogle is the nearest
/// entity named Moogle here; retail finds it by model 0x52 / 0x3d5 / 0x58a / 0x939, inferred stand-in).
pub fn line(w: &mut World, n: u16) {
    let Some(bt) = w.battle.clone() else { return };
    let name = w
        .entities
        .values()
        .filter(|e| e.name.contains("Moogle"))
        .min_by(|a, b| (a.pos - w.pos).length().total_cmp(&(b.pos - w.pos).length()))
        .map_or_else(|| "Moogle".to_string(), |e| e.name.clone());
    let ctx = crate::dat::battle::Ctx { actor: None, target: None, me: w.name.clone(), work: [0; 5] };
    if let Some(l) = bt.system(n, &ctx)
        && let Some(text) = l.lines.into_iter().find(|l| !l.is_empty())
    {
        w.chat.push(format!("{name} : {text}"));
    }
}
