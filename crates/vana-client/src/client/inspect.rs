//! /check on a player (spec ~/ffxi/lab/research/check_spec.md): s2c 0x0CA (title line, bazaar message) and 0x0C9
//! (equipment, jobs, linkshell; handler LAB_100e5af0) fill the store at G+0x19adc; the first 0x0C9 opens
//! "menu    inspect" (DAT_105781b8, vtable 0x103337e0) with the item info following the cursor.

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;
use crate::inventory::SLOT_CHILD;

pub const WINDOW: &str = "menu    inspect";
const COMMENT: &str = "menu    comment";
/// Child 17: View Wares.
const VIEW: u16 = 17;
/// View Wares stays disabled 2 s after the window gets the focus back (DAT_105781c2 = 0x78 ticks).
const VIEW_DELAY: f32 = 120.0;

/// 0x0C9 kind 1 (G+0x19d28): jobs, levels, master level, the linkshell.
#[derive(Clone, Debug, Default)]
pub struct General {
    pub jobs: [u8; 2],
    pub levels: [u8; 2],
    pub mlvl: u8,
    pub levelling: bool,
    pub ls_name: [u8; 16],
    pub ls_colour: u16,
    pub ls_item: u16,
}

/// The store, the bazaar state (DAT_105781c0 / c1 / c2), the comment lines and the window.
#[derive(Default)]
pub struct Inspect {
    /// The check's UniqueNo / ActIndex (G+0x19d20 / +0x19d1c).
    pub expect: (u32, u16),
    /// Packets taken (G+0x19d24 without bit 7).
    pub counter: u8,
    /// Items by equipment slot (0x10 = empty in retail) with their 24 data bytes.
    pub slots: [Option<(u16, [u8; 24])>; 16],
    pub info: Option<General>,
    pub bazaar: bool,
    pub msg_in: bool,
    pub delay: f32,
    /// The checked player's bazaar message, 3 lines of 40.
    pub comment: [String; 3],
    pub win: Option<Win>,
    /// +0x148: the previous event child, kept across opens.
    prev: u16,
    /// +0x14e: a typed check made without the action menu up: closing does not bring the menu back.
    no_reopen: bool,
    last: Option<std::time::Instant>,
}

pub struct Win {
    pub cursor: u16,
    /// +0x14a: the column last left.
    col: u16,
}

impl Inspect {
    pub fn up(&self) -> bool {
        self.win.is_some()
    }

    /// FUN_100e55f0: every record empty, ids, counter and info cleared.
    fn reset(&mut self) {
        (self.slots, self.expect, self.counter, self.info) = (Default::default(), (0, 0), 0, None);
    }

    /// FUN_10133a80's enable rule: a bazaar and the 0x0CA in, or the focus delay run out.
    pub fn view_enabled(&self) -> bool {
        self.bazaar && (self.msg_in || self.delay < 0.0)
    }
}

/// FUN_101347b0's part for the store: a check is going out to (id, index); `fresh` when no check is pending (c4 == 0)
/// resets the records; `typed` for /check, /checkname and Ctrl+C.
pub fn sent(w: &mut World, id: u32, index: u16, fresh: bool, typed: bool) {
    let menu = w.action_menu.is_some();
    let s = &mut w.inspect;
    if fresh {
        s.reset();
    }
    (s.expect, s.counter, s.info, s.bazaar) = ((id, index), 0, None, false);
    s.no_reopen = typed && !menu;
}

/// The main-window class (registry flag 0x20000, FUN_1015ecc0): while one is up the inspect window does not open.
fn blocked(w: &World) -> bool {
    w.win.is_some() || w.map.is_some() || w.config.is_some() || w.settings.is_some() || w.search.is_some() || w.magic.is_some() || w.ability.is_some() || w.moogle.is_some() || super::shop::active(w) || super::trade::active(w) || w.inspect.up()
}

/// s2c 0x0C9 (LAB_100e5af0): only for the pending check; the first opens the window (FUN_10134650).
pub fn on_equip(w: &mut World, p: &[u8]) {
    let u8_at = |o: usize| p.get(o).copied().unwrap_or(0);
    let u16_at = |o: usize| u16::from_le_bytes([u8_at(o), u8_at(o + 1)]);
    let u32_at = |o: usize| u32::from_le_bytes([u8_at(o), u8_at(o + 1), u8_at(o + 2), u8_at(o + 3)]);
    if (u32_at(4), u16_at(8)) != w.inspect.expect || w.inspect.expect.0 == 0 {
        return;
    }
    if w.inspect.counter == 0 {
        open(w);
    }
    let s = &mut w.inspect;
    match u8_at(0xa) {
        0 => {
            let slot = u8_at(0xe) as usize;
            if slot < 16 {
                s.slots[slot] = Some((u16_at(0xc), [0; 24]));
            }
            s.counter = s.counter.wrapping_add(1);
        }
        1 => {
            let ls_name = std::array::from_fn(|j| u8_at(0x10 + j));
            s.info = Some(General {
                jobs: [u8_at(0x22), u8_at(0x23)],
                levels: [u8_at(0x24), u8_at(0x25)],
                mlvl: u8_at(0x27),
                levelling: u8_at(0x28) & 1 != 0,
                ls_name,
                ls_colour: u16_at(0x20) & 0xfff,
                ls_item: u16_at(0xe),
            });
        }
        2 => {
            for k in 0..8 {
                let item = u16_at(0xc + 2 * k);
                if item == 0 {
                    break;
                }
                let slot = u8_at(0x1c + k) as usize;
                if slot < 16 {
                    s.slots[slot] = Some((item, [0; 24]));
                }
            }
            s.counter = s.counter.wrapping_add(1);
        }
        3 => {
            for k in 0..u8_at(0xb) as usize {
                let o = 0xc + 0x1c * k;
                let item = u16_at(o);
                if item == 0 {
                    break;
                }
                let slot = u8_at(o + 2) as usize;
                let mut data = [0u8; 24];
                for (j, b) in data.iter_mut().enumerate() {
                    *b = u8_at(o + 4 + j);
                }
                if slot < 16 {
                    s.slots[slot] = Some((item, data));
                }
            }
            s.counter = s.counter.wrapping_add(1);
        }
        _ => {}
    }
}

/// FUN_10134670(-1, 1): refused (the check forgotten, the data kept) while a main-class window is up; else the window
/// opens focused on View Wares and the action menu closes (+0x44).
fn open(w: &mut World) {
    if blocked(w) {
        w.last_check = None;
        return;
    }
    w.action_menu = None;
    // Focus gained (FUN_101337d0): View Wares waits 2 s unless the 0x0CA is in.
    w.inspect.delay = VIEW_DELAY;
    w.inspect.win = Some(Win { cursor: VIEW, col: 0 });
}

/// s2c 0x0CA: the title line (group 10 0x33 in mode 0x29, not for ourselves), the bazaar flag from the target's 0x00D
/// (actor +0x128 bit 9), "arrived", and the message cut into 3 lines of 40.
pub fn on_message(w: &mut World, p: &[u8]) {
    // MyFlag (@0x7f bit 1): our own message, also sent at every zone-in (LAB_10133870 -> FUN_101ddae0).
    if p.get(0x7f).is_some_and(|b| b & 2 != 0) {
        super::seacom::own_message(w, p.get(4..4 + 120).unwrap_or(&[]));
        if w.inspect.expect.0 != w.my_id {
            return;
        }
    }
    let text = |a: usize, n: usize| -> String {
        let b: Vec<u8> = p.get(a..(a + n).min(p.len())).unwrap_or(&[]).iter().copied().take_while(|&c| c != 0).collect();
        crate::dat::font::decode_sjis(&b)
    };
    let name = text(0x80, 16);
    let title = p.get(0x90..0x94).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
    if name != w.name
        && let Some(bt) = w.battle.clone()
    {
        let t = w.names.as_ref().and_then(|n| crate::dat::names::NameSource::title(&**n, title)).unwrap_or_default();
        let fmt = bt.group10(0x33).unwrap_or_default();
        let line = fmt.replacen("%s", &name, 1).replacen("%s", &t, 1);
        w.chat.push_colour(line, bt.colour_in(0x29, &w.game));
    }
    let id = w.inspect.expect.0;
    w.inspect.bazaar = w.entities.get(&id).is_some_and(|e| e.marks.bazaar);
    w.inspect.msg_in = true;
    let raw: Vec<u8> = p.get(4..4 + 120).unwrap_or(&[]).to_vec();
    w.inspect.comment = std::array::from_fn(|k| {
        let line: Vec<u8> = raw.iter().skip(40 * k).take(40).copied().take_while(|&c| c != 0).collect();
        crate::dat::font::decode_sjis(&line).trim_end().to_string()
    });
}

/// Once a frame: the View Wares delay in 60 Hz ticks (FUN_10014cf0, at least 1).
pub fn frame(w: &mut World) {
    let now = std::time::Instant::now();
    let dt = w.inspect.last.map_or(0.0, |t| (now - t).as_secs_f32());
    w.inspect.last = Some(now);
    if w.inspect.delay >= 0.0 && w.inspect.up() {
        w.inspect.delay -= (dt * 60.0).max(1.0);
    }
}

/// FUN_10134760 / FUN_101336d0: the window and the item info go, a new check may go at once, and the action menu
/// comes back unless the check was typed without it.
pub fn close(w: &mut World) {
    if w.inspect.win.take().is_none() {
        return;
    }
    w.last_check = None;
    if !w.inspect.no_reopen && w.action_menu.is_none() {
        w.action_menu = Some(super::combat::open_menu(w));
    }
}

/// The broadcasts 0x8 / 0x10 / 0x20 / 0x800 (zoning, target dropped, engaged, an event): closed and the store reset.
pub fn drop(w: &mut World) {
    if w.inspect.up() {
        close(w);
        w.inspect.reset();
    }
}

/// The generic handler (SE 0001 on a move, 0002 / 0004 on Enter, 0003 on Escape) then FUN_10134560: Up / Down off View
/// Wares land in the remembered column.
pub fn key(w: &mut World, gu: &GameUi, key: egui::Key) {
    use egui::Key as K;
    let enabled = w.inspect.view_enabled();
    let Some(win) = w.inspect.win.as_mut() else { return };
    let link = |k: usize| gu.menu_links(WINDOW, win.cursor).map(|l| l[k] as u16);
    let moved = match key {
        K::ArrowUp => link(2),
        K::ArrowDown => link(3),
        K::ArrowRight => link(4),
        K::ArrowLeft => link(5),
        K::Enter => {
            // View Wares would open the bazaar list (c2s 0x105, "menu    shop"): not done.
            let sound = if win.cursor == VIEW && !enabled { Ui::Error } else { Ui::Select };
            w.sounds.push(sound as u32);
            return;
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            close(w);
            return;
        }
        _ => return,
    };
    let Some(mut next) = moved.filter(|&c| c != 0) else { return };
    if w.inspect.prev == VIEW {
        next = (win.col * 4 + next).clamp(1, VIEW);
    } else if next != VIEW {
        win.col = (next - 1) / 4;
    }
    w.inspect.prev = next;
    if next != win.cursor {
        win.cursor = next;
        w.sounds.push(Ui::Cursor as u32);
    }
}

/// The mouse: the generic window over the slots and View Wares (winhits.rs).
pub fn pointer(w: &mut World, gu: &GameUi, p: (f32, f32), moved: bool, clicked: bool, right: bool) {
    // The modern layer draws it and takes its mouse.
    if w.modern {
        return;
    }
    let Some(cursor) = w.inspect.win.as_ref().map(|win| win.cursor) else { return };
    super::winhits::generic(w, gu, WINDOW, cursor, 1..=VIEW, p, moved, clicked, right, |w, id| {
        w.inspect.prev = id;
        if let Some(win) = w.inspect.win.as_mut() {
            if id != VIEW {
                win.col = (id - 1) / 4;
            }
            win.cursor = id;
        }
    }, key);
}

/// The cursor slot's item for the item info window (mode 0): none on View Wares (the comment window shows there).
pub fn info_source(w: &World) -> Option<(u16, Option<[u8; 24]>)> {
    let win = w.inspect.win.as_ref()?;
    let slot = SLOT_CHILD.iter().position(|&c| c == win.cursor)?;
    let (item, data) = w.inspect.slots[slot]?;
    Some((item, Some(data)))
}

/// Title slot 2: the checked player's name; help slot 2: "Lv.%d %s" (or "M.Lv.%d %s") and " / " the sub job, or
/// "？　？　？" when anonymous (FUN_10133a80).
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    if !w.inspect.up() {
        return;
    }
    bars.title[2] = super::textcmd::display_name(w, w.inspect.expect.0);
    let Some(g) = &w.inspect.info else { return };
    if g.jobs[0] == 0 {
        bars.help[2] = "？　？　？".into();
        return;
    }
    let lv = |fmt: usize, level: u8, job: u8| gu.lobby_msg(10, fmt, "Lv.%d %s").replacen("%d", &level.to_string(), 1).replacen("%s", &gu.lobby_msg(0x12, job as usize, ""), 1);
    let mut line = if g.levelling { lv(0x79, g.mlvl, g.jobs[0]) } else { lv(0x58, g.levels[0], g.jobs[0]) };
    if g.jobs[1] != 0 {
        line.push_str(" / ");
        line.push_str(&lv(0x58, g.levels[1], g.jobs[1]));
    }
    bars.help[2] = line;
}

/// FUN_10134230: the window (slot words, the cursor child tinted), the icons at scale 1 without counts, anc_equi on a
/// slot or the hand on View Wares (#431 when disabled), the linkshell line; the comment window while on View Wares.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    const WHITE: u32 = 0x8080_8080;
    let mut rects = Vec::new();
    if w.modern {
        return rects;
    }
    let (Some(win), Some(info)) = (&w.inspect.win, w.item_info.clone()) else { return rects };
    let enabled = w.inspect.view_enabled();
    let off: &[u16] = if enabled { &[] } else { &[VIEW] };
    let on_view = win.cursor == VIEW;
    let Some(lw) = gu.lobby_window_off(WINDOW, sw, sh, (0.0, 0.0), Some(win.cursor), on_view && enabled, off) else { return rects };
    super::winhits::record(w, WINDOW, &lw);
    let [x0, y0, ..] = lw.frame;
    if !enabled {
        gu.disabled_children(WINDOW, x0, y0, off, Some(win.cursor));
    }
    let mut by_child = vec![(0u16, 0u32, false); 16];
    for (slot, s) in w.inspect.slots.iter().enumerate() {
        if let Some((item, _)) = s {
            by_child[SLOT_CHILD[slot] as usize - 1] = (*item, 0, true);
        }
    }
    super::post::draw_slots(gu, &info, WINDOW, lw.frame, &by_child, None, Some(win.cursor), |_| [1.0; 4]);
    if !on_view
        && let Some(e) = gu.layout(WINDOW).and_then(|l| l.elements.iter().find(|e| e.id == win.cursor).cloned())
    {
        let n = gu.lib_len("anc     anc_equi").max(1);
        gu.sprite("anc     anc_equi", gu.cursor_frame() % n, x0 + e.x as f32, y0 + e.y as f32, WHITE);
    }
    if let Some(g) = &w.inspect.info
        && g.ls_item != 0
    {
        let name = super::linkshell::ls_name(&g.ls_name);
        if !name.is_empty() {
            super::items::upload_icons(gu, &info, std::iter::once(g.ls_item));
            let icon = crate::render::invwin::icon_texture(g.ls_item);
            let c = g.ls_colour as u32;
            let ch = |s: u32| (((c >> s) & 0xf) * 13 + 0x38) as f32 / 128.0;
            if gu.has_image(&icon) {
                gu.image_rect(&icon, x0 + 16.0, y0 + 162.0, 16.0, 16.0, [ch(0), ch(4), ch(8), 1.0]);
            }
            gu.shape_fit(&name, x0 + 36.0, y0 + 166.0, 140.0, WHITE);
        }
    }
    rects.push(lw.frame);
    if on_view && let Some(cw) = gu.lobby_window_off(COMMENT, sw, sh, (0.0, 0.0), None, false, &[]) {
        for (k, line) in w.inspect.comment.iter().enumerate() {
            if !line.is_empty() {
                gu.text(line, cw.frame[0] + 4.0, cw.frame[1] + 4.0 + 16.0 * k as f32, 0, crate::dat::battle::argb_bytes(WHITE));
            }
        }
        rects.push(cw.frame);
    }
    rects
}

/// The window's cursor onto child `k` (a slot, or View Wares) as the mouse moves it. True when it moved.
pub(super) fn set_cursor(w: &mut World, k: u16) -> bool {
    let Some(win) = w.inspect.win.as_mut().filter(|_| (1..=VIEW).contains(&k)) else { return false };
    if win.cursor == k {
        return false;
    }
    if k != VIEW {
        win.col = (k - 1) / 4;
    }
    (win.cursor, w.inspect.prev) = (k, k);
    true
}

/// View Wares' child and whether it can be pressed now.
pub(super) fn view_wares(w: &World) -> (u16, bool) {
    (VIEW, w.inspect.view_enabled())
}
