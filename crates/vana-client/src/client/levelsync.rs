//! "menu    levelsyn" (party menu Level Sync; spec ~/ffxi/lab/research/party_menu_spec.md 3): a second results window
//! (DAT_106643b4, vtable 0x1033af48: open FUN_10204880, close FUN_102048f0, rows FUN_10204940, Enter FUN_10204af0)
//! over the party's group list, to pick whom to synchronise levels with.

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;
use crate::net::search::{Record, Reply};

const WIN: &str = "menu    levelsyn";
/// The rows the window shows at once.
const ROWS: usize = 15;

pub struct LevelSync {
    pub busy: bool,
    pub rows: Vec<Record>,
    pub cursor: usize,
    pub scroll: usize,
}

/// LAB_10204da0 (the s2c 0x0E1 callback): the group list of our party id into the window; title slot 2 0xd 0x95.
pub fn open(w: &mut World, party_id: u32) {
    if w.levelsync.as_ref().is_some_and(|l| l.busy) {
        return;
    }
    log::info!("level sync: group list of party {party_id:#x}");
    let rx = crate::net::search::spawn(w.search_addr, crate::net::search::Cond::group([party_id, 0, 0, 0]));
    w.searches.push((rx, super::search::Then::LevelSync));
    w.levelsync = Some(LevelSync { busy: true, rows: Vec::new(), cursor: 0, scroll: 0 });
}

/// The reply's rows: FUN_10204940 keeps the members of our six slots (FUN_102031f0(id, 1, 1), trusts too); the header
/// and group-row slots return 0 (no header row).
pub fn filled(w: &mut World, reply: Option<Reply>) {
    let six: Vec<u32> = w.party.slots().slots[..6].iter().flatten().map(|m| m.id).chain([w.my_id]).collect();
    let Some(l) = w.levelsync.as_mut() else { return };
    l.busy = false;
    l.rows = reply.map_or(Vec::new(), |r| r.records.into_iter().filter(|r| six.contains(&r.id)).collect());
}

/// FUN_10203190: the first member of our party with level sync on (GAttr 0x100).
pub fn synced(w: &World) -> Option<&super::party::Member> {
    w.party.party().find(|m| m.flags & 0x100 != 0)
}

/// FUN_102048f0: a pending prompt of its own is dropped, the window closes, title slot 2 cleared.
pub fn close(w: &mut World) {
    if matches!(w.confirm, Some((super::Confirm::Sync { .. }, _))) {
        w.confirm = None;
    }
    w.levelsync = None;
}

/// The list base keys: Up / Down stop at the ends, Left / Right page by 15, Escape closes; Enter FUN_10204af0: under
/// level 10 SE 0004, else group 10 0x72 (the synced member: off) or 0x71 (on) to confirm.
pub fn key(w: &mut World, key: egui::Key) {
    use egui::Key as K;
    let Some(l) = w.levelsync.as_mut() else { return };
    let n = l.rows.len();
    match key {
        K::ArrowDown if l.cursor + 1 < n => {
            l.cursor += 1;
            w.sounds.push(Ui::Cursor as u32);
        }
        K::ArrowUp if l.cursor > 0 => {
            l.cursor -= 1;
            w.sounds.push(Ui::Cursor as u32);
        }
        K::ArrowRight | K::ArrowLeft if n > 0 => {
            let c = if key == K::ArrowRight { (l.cursor + ROWS).min(n - 1) } else { l.cursor.saturating_sub(ROWS) };
            if c != l.cursor {
                l.cursor = c;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::Escape => {
            w.sounds.push(Ui::Cancel as u32);
            close(w);
            return;
        }
        K::Enter => {
            let Some(r) = l.rows.get(l.cursor).cloned() else { return };
            if r.level.0 < 10 {
                w.sounds.push(Ui::Error as u32);
                return;
            }
            w.sounds.push(Ui::Select as u32);
            let off = synced(w).is_some_and(|m| m.name == r.name);
            w.confirm = Some((super::Confirm::Sync { id: r.id, race: r.race, off }, false));
        }
        _ => {}
    }
    if let Some(l) = w.levelsync.as_mut() {
        l.scroll = l.scroll.min(l.cursor).max((l.cursor + 1).saturating_sub(ROWS));
    }
}

/// The mouse's cursor move onto row `k`. True when it moved.
pub(super) fn set_cursor(w: &mut World, k: usize) -> bool {
    let Some(l) = w.levelsync.as_mut().filter(|l| l.cursor != k && k < l.rows.len()) else { return false };
    l.cursor = k;
    l.scroll = l.scroll.min(k).max((k + 1).saturating_sub(ROWS));
    true
}

/// The prompt's question (FUN_10153180 with the name and the pronoun by race, FUN_101cc590).
pub fn ask_text(w: &World, id: u32, race: u8, off: bool) -> String {
    let Some(bt) = w.battle.as_ref() else { return String::new() };
    let Some(raw) = bt.group10_raw(if off { 0x72 } else { 0x71 }) else { return String::new() };
    let sjis = crate::dat::font::encode_sjis(&name_of(w, id));
    let p = super::partymenu::pronoun(race as u16);
    crate::dat::font::decode_sjis(&crate::dat::msgfmt::format(&raw, &[&sjis], &[p], &|_, _, _| None))
}

fn name_of(w: &World, id: u32) -> String {
    let row = w.levelsync.as_ref().and_then(|l| l.rows.iter().find(|r| r.id == id)).map(|r| r.name.clone());
    row.or_else(|| w.party.members().find(|m| m.id == id).map(|m| m.name.clone())).unwrap_or_default()
}

/// Yes (DAT_10204a50 / DAT_10204aa0, LAB_10204e00): c2s 0x077 {name, 0, 6 on / 7 off}; levelsyn closes.
pub fn answer(w: &mut World, id: u32, off: bool) {
    let name = name_of(w, id);
    let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x077, body: super::party::change_body(&name, 0, if off { 7 } else { 6 }) });
    w.levelsync = None;
}

/// The window (layout as scresult's, windowps 72) and its rows: col 0 (x 1) keytops3 471 before the synced member,
/// col 1 (x 18) the job / level cell, col 2 (x 160) the name, grey 0x80404040 under level 10 or anonymous (0x4000),
/// col 3 (x 270) the zone name when not ours (column table 0x10385378); the cursor bar windowps 75.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    const G: u32 = 0x8080_8080;
    let mut rects = Vec::new();
    let Some(l) = &w.levelsync else { return rects };
    let focused = w.confirm.is_none();
    let sel = if focused && !l.rows.is_empty() { (l.cursor - l.scroll + 1) as u16 } else { 0 };
    let Some((wx, wy)) = gu.menu_sel(WIN, sw, sh, sel) else { return rects };
    rects.extend(gu.window_rect(WIN, sw, sh));
    let rx = wx + 4.0;
    let synced = synced(w).map(|m| m.name.clone());
    for i in l.scroll..l.rows.len().min(l.scroll + ROWS) {
        let ry = wy + 4.0 + 16.0 * (i - l.scroll) as f32;
        let r = &l.rows[i];
        if synced.as_ref() == Some(&r.name) {
            gu.sprite("menu    keytops3", 471, rx + 1.0, ry + 2.0, G);
        }
        super::search::job_cell(gu, r, rx + 18.0, ry + 2.0, G, false);
        let nc = if r.level.0 < 10 || r.flags & 0x4000 != 0 { 0x8040_4040 } else { G };
        gu.shape_fit(&r.name, rx + 160.0, ry + 2.0, 100.0, nc);
        if r.zone != w.pos_zone {
            let s = crate::dat::font::decode_sjis(&gu.area_name(r.zone));
            gu.shape_text(&s, rx + 270.0, ry + 2.0, G, false);
        }
    }
    if focused && !l.rows.is_empty() {
        gu.sprite("menu    windowps", 75, wx, wy + 4.0 + 16.0 * (l.cursor - l.scroll) as f32, G);
    }
    gu.scroll_bar(wx + 358.0, wy, 248.0, l.rows.len(), ROWS, l.scroll);
    rects
}

/// Help slot 1 and title from the frame (0xe 0x2a2, 0xd 0x95); title slot 2 0xd 0x95 as the callback sets it.
pub fn bars(gu: &GameUi, w: &World, bars: &mut super::hudtop::BarText) {
    let Some(l) = &w.levelsync else { return };
    let [help, title] = gu.focus_text(WIN, (l.cursor - l.scroll + 1) as u16);
    (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    bars.help[2] = String::new();
    bars.title[2] = gu.title_msg(0x95).unwrap_or_default();
}
