//! Records of Eminence lists (roe_lists_spec.md): quest01 over quest00 in modes 2 Current, 3 Objective List (the
//! category tree of dat/roe.rs) and 4 Vana'Bout, the description with its rewards, and the prompts that send c2s
//! 0x10C (set), 0x10D (remove) and 0x10E (claim).

use super::World;
use super::game_ui::GameUi;
use super::net::NetCommand;
use super::quests::{LIST, Roe};
use crate::audio::Ui;
use crate::dat::roe::{Category, Objective, ROOT};
use std::time::{Duration, Instant};

const VISIBLE: usize = 8;
const WHITE: u32 = 0x8080_8080;
const CURSOR: u32 = 0x8080_3040;
const GREY: u32 = 0x8040_4040;
/// FUN_10232760: DAT_10668edc > 29, the frames since the open or the last send (30 frames, 1 s at 30 fps *(inf)*).
const COOLDOWN: Duration = Duration::from_secs(1);

#[derive(Clone, Debug)]
pub struct List {
    /// 2 Current, 3 Objective List, 4 Vana'Bout.
    pub mode: usize,
    /// 0 the list, 1 a description.
    pub state: u8,
    pub rows: Vec<u16>,
    /// +0x8e / +0x8f (modes 2 / 4), +0x74 (mode 3): the rows are built.
    built: bool,
    pub top: usize,
    pub cursor: usize,
    /// +0x7c / +0x86: the categories walked (the root first) and the cursor each one was left on.
    stack: Vec<(u16, usize)>,
    /// +0x78 / +0x7a: the next and previous sibling categories (mode 3 below the root).
    siblings: (u16, u16),
    /// +0x8c.
    described: u16,
    /// +0x90: the second description page.
    page2: bool,
    /// DAT_10668edc: the last send (none: 30 at the setup).
    sent: Option<Instant>,
}

/// The comyn prompts of FUN_10232770 and LAB_10232970, on objective number `n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ask {
    pub kind: AskKind,
    pub n: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskKind {
    /// 0xc 0x6f "Set objective?" (LAB_10232ab0).
    Set,
    /// 0xc 0x73 "Remove objective?" (LAB_10232970).
    Remove,
    /// 0xc 0x74 "Your progress will be reset ..." (FUN_10210f10, LAB_10232a40).
    Reset,
    /// 0xc 0x75 "Receive rewards?" (LAB_10232ae0).
    Claim,
}

fn num(id: u16) -> u32 {
    id.wrapping_add(0x2000) as u32
}

impl Roe {
    /// FUN_102308a0: one of the 64 records holds the objective.
    fn is_active(&self, id: u16) -> bool {
        self.flags & 2 != 0 && id >= 0xe000 && self.active.iter().any(|r| r & 0xfff == num(id))
    }

    /// FUN_10230a80: the first such record's count.
    fn progress(&self, id: u16) -> u32 {
        if self.flags & 2 == 0 {
            return 0;
        }
        self.active.iter().find(|r| *r & 0xfff == num(id)).map_or(0, |r| r >> 12)
    }

    /// FUN_10230930.
    fn claimable(&self, id: u16, o: Option<&Objective>) -> bool {
        self.is_active(id) && o.is_some_and(|o| self.progress(id) >= o.goal)
    }

    /// FUN_102308e0: the objective's bit in the 0x112 log.
    fn completed_id(&self, id: u16) -> bool {
        let n = num(id) as usize;
        self.flags & 4 != 0 && id >= 0xe000 && self.log.get(n / 8).is_some_and(|b| b >> (n % 8) & 1 != 0)
    }

    /// FUN_102309d0: completed and not repeatable.
    fn done(&self, id: u16, o: Option<&Objective>) -> bool {
        self.completed_id(id) && o.is_some_and(|o| !o.repeatable)
    }

    /// FUN_102309b0: slots 0..29 all taken.
    fn full(&self) -> bool {
        self.active[..30].iter().all(|r| r & 0xfff != 0)
    }

    /// FUN_10230ad0: slots 0..29 taken.
    fn slots(&self) -> usize {
        if self.flags & 2 == 0 { 0 } else { self.active[..30].iter().filter(|r| *r & 0xfff != 0).count() }
    }

    /// FUN_100990c0 / FUN_100990d0: slot 63's (the timed record) or slot 42's objective.
    fn timed(&self, id: u16) -> bool {
        [63, 42].iter().any(|&s| (self.active[s] & 0xfff) as u16 + 0xe000 == id)
    }

    /// FUN_10230ba0: n in Vana'Bout slots 43..52.
    fn vanabout_slot(&self, n: u16) -> bool {
        self.active[43..53].iter().any(|r| r & 0xfff == n as u32)
    }
}

/// FUN_10230f60's inputs.
struct Ctx<'a> {
    roe: &'a Roe,
    levels: [u8; 24],
    zones: Option<&'a [u8; 0x30]>,
    skills: Option<&'a [u16; 64]>,
    unity: u8,
}

fn ctx(w: &World) -> Ctx<'_> {
    Ctx {
        roe: &w.roe,
        levels: w.job_info.map_or([0; 24], |j| j.levels),
        zones: w.known_zones.as_ref(),
        skills: w.skill_base.as_ref(),
        unity: if w.recasts_seen { w.stats.as_ref().map_or(0, |s| (s.unity & 0x1f) as u8) } else { 0 },
    }
}

/// FUN_10230f60: all four conditions {b0, b1, b2, type} pass; an unknown type fails.
fn passes(c: &Ctx, conds: &[[u8; 4]; 4]) -> bool {
    let level = |j: u8| c.levels.get(j as usize).copied().unwrap_or(0);
    conds.iter().all(|b| {
        let v16 = u16::from_le_bytes([b[0], b[1]]);
        match b[3] {
            0 => true,
            1 => c.roe.completed_id(v16.wrapping_add(0xe000)),
            2 => {
                let m = u32::from_le_bytes([b[0], b[1], b[2], 0]);
                (0..22).all(|j| m >> j & 1 == 0 || level(j as u8 + 1) != 0)
            }
            3 => b[0] <= level(b[1]),
            4 => v16 < 0x180 && c.zones.is_some_and(|k| k[v16 as usize >> 3] >> (v16 & 7) & 1 != 0),
            5 => {
                let s = c.skills.and_then(|s| s.get(b[2] as usize)).copied().unwrap_or(0);
                !(s & 0x8000 != 0 && s & 0x7fff == 0) && v16 <= s & 0x7fff
            }
            6 => c.roe.flags & 4 != 0 && c.roe.rot.get(b[1] as usize) == Some(&b[0]),
            7 => b[0] != 0 && c.unity == b[0],
            8 => b[0] != 0 && (1..=22).any(|j| level(j) <= b[0]),
            _ => false,
        }
    })
}

/// FUN_10231000: the children that are set and pass, in record order.
fn visible(c: &Ctx, cat: &Category) -> Vec<u16> {
    cat.kids.iter().filter(|k| k.0 != 0 && k.0 != 0xe000 && passes(c, &k.1)).map(|k| k.0).collect()
}

/// FUN_102305e0 / FUN_10230510 / FUN_10230580: the next and previous set, passing entries of the parent from the
/// current one (wrapping, itself excluded); none when the parent has fewer than 2.
fn siblings(c: &Ctx, parent: &Category, cur: u16) -> (u16, u16) {
    let k = &parent.kids;
    let n = k.len();
    if n < 2 {
        return (0, 0);
    }
    let i = k.iter().position(|x| x.0 == cur).unwrap_or(0);
    let pick = |j: usize| (k[j].0 != 0 && passes(c, &k[j].1)).then_some(k[j].0);
    let next = (1..n).find_map(|d| pick((i + d) % n)).unwrap_or(0);
    let prev = (1..n).find_map(|d| pick((i + n - d) % n)).unwrap_or(0);
    (next, prev)
}

/// The 0x111 handler (0x100990e0): a limited-time record in slot 63 other than the one last seen queues "New
/// Challenge"; the id is kept either way (reset only at login, so a zone change with the same record shows nothing).
pub fn timed_seen(w: &mut World) {
    let id = (w.roe.active[63] & 0xfff) as u16;
    if id != 0 && id != w.roe_timed {
        w.new_challenge = true;
    }
    w.roe_timed = id;
}

/// FUN_10087230 each frame: once the zone is in, no event runs and our model is loaded, "New Challenge" (file 0xaf0 +
/// 0x396, en / ja; fr 0x397, de 0x398: table 0x1035b53c) `main` on us, with no text.
pub fn new_challenge(w: &mut World) {
    if !w.new_challenge || !w.in_scene() || w.zone_uploading.is_some() || super::event::active(w) || !w.actors.get(&w.my_id).is_some_and(|a| a.character.is_some()) {
        return;
    }
    w.new_challenge = false;
    log::info!("RoE: New Challenge, limited-time record {}", w.roe_timed);
    let me = crate::fx::sched::Who::Actor(w.my_id);
    w.effects.on_event_scheduler(0xaf0 + 0x396, u32::from_le_bytes(*b"main"), me, me);
}

fn objective(w: &World, id: u16) -> Option<std::sync::Arc<Objective>> {
    w.item_info.as_ref().and_then(|c| c.objective(id))
}

/// quest00 rows 2 / 3 / 4 (FUN_10230740 -> FUN_10231550 / FUN_10231610 / FUN_102316e0): quest01 in that mode, the
/// rows built on its first frame.
pub fn open(w: &mut World, mode: usize) {
    open_hook(w);
    let l = List { mode, state: 0, rows: Vec::new(), built: false, top: 0, cursor: 0, stack: vec![(ROOT, 0)], siblings: (0, 0), described: 0, page2: false, sent: None };
    if let Some(q) = w.quests.as_mut() {
        q.roe = Some(l);
    }
}

/// quest01's open hook (FUN_102326a0 -> FUN_100871a0(0), every mode): after a 0x110, c2s 0x117 {0} asks again.
pub fn open_hook(w: &mut World) {
    if w.roe.flags & 1 != 0 {
        let _ = w.net.commands.send(NetCommand::Packet { id: 0x117, body: vec![0; 4] });
        w.roe.flags &= !1;
    }
}

/// LAB_1022f370 on s2c 0x111: modes 2 / 4 build again (a description waits for the list).
pub fn active_changed(w: &mut World) {
    if let Some(l) = w.quests.as_mut().and_then(|q| q.roe.as_mut())
        && l.mode != 3
    {
        l.built = false;
    }
}

fn show(l: &mut List) {
    l.cursor = l.cursor.min(l.rows.len().saturating_sub(1));
    if l.cursor < l.top {
        l.top = l.cursor;
    } else if l.cursor >= l.top + VISIBLE {
        l.top = l.cursor + 1 - VISIBLE;
    }
    l.top = l.top.min(l.rows.len().saturating_sub(VISIBLE));
}

/// FUN_10231080: mode 3's rows from the current category once 0x110, 0x111 and the whole 0x112 are in (else it
/// waits, and builds again from row 0 once they are).
fn build_tree(w: &World, l: &mut List) {
    if w.roe.flags & 7 != 7 {
        l.built = false;
        return;
    }
    if l.built {
        return;
    }
    let Some(cat) = w.item_info.as_ref().and_then(|c| c.category(l.stack.last().map_or(ROOT, |s| s.0))) else { return };
    (l.rows, l.built, l.cursor, l.top) = (visible(&ctx(w), &cat), true, 0, 0);
}

/// Per frame (the row callbacks): modes 2 / 4 (FUN_10230bd0 / FUN_10230d00, the cursor kept) and 3, the siblings.
pub fn update(w: &mut World) {
    let Some(mut l) = w.quests.as_ref().and_then(|q| q.roe.clone()) else { return };
    let r = &w.roe;
    match l.mode {
        2 | 4 if !l.built && r.flags & 2 != 0 && l.state == 0 => {
            let slots: Vec<usize> = if l.mode == 2 { [63, 42].into_iter().chain(0..30).collect() } else { (43..63).collect() };
            l.rows = slots.into_iter().map(|s| (r.active[s] & 0xfff) as u16).filter(|&n| n != 0).map(|n| 0xe000 + n).collect();
            l.built = true;
            show(&mut l);
        }
        3 => build_tree(w, &mut l),
        _ => {}
    }
    l.siblings = (0, 0);
    if l.mode == 3 && l.state == 0 && l.stack.len() > 1 {
        let cur = l.stack[l.stack.len() - 1].0;
        if let Some(parent) = w.item_info.as_ref().and_then(|c| c.category(l.stack[l.stack.len() - 2].0)) {
            l.siblings = siblings(&ctx(w), &parent, cur);
        }
    }
    if let Some(q) = w.quests.as_mut() {
        q.roe = Some(l);
    }
}

fn ready(l: &List) -> bool {
    l.sent.is_none_or(|t| t.elapsed() >= COOLDOWN)
}

/// FUN_10232b40: the description, then at once its prompt.
fn describe(w: &mut World, l: &mut List, id: u16) {
    if !ready(l) {
        return;
    }
    (l.described, l.state, l.page2) = (id, 1, false);
    prompt(w, l);
}

/// FUN_10232770: Receive rewards? (mode 2, claimable, Yes), Remove objective? (mode 2, No; mode 4 in slots 43..52,
/// Yes), Set objective? (mode 3: not timed, not taken, a free slot; Yes); none for a timed record, a full log or one
/// done for good.
fn prompt(w: &mut World, l: &List) {
    if !ready(l) {
        return;
    }
    let id = l.described;
    let n = id.wrapping_sub(0xe000);
    let o = objective(w, id);
    let r = &w.roe;
    let (kind, yes) = match l.mode {
        2 if r.claimable(id, o.as_deref()) => (AskKind::Claim, true),
        2 if r.timed(id) => return,
        2 => (AskKind::Remove, false),
        3 if r.timed(id) || r.is_active(id) || r.slots() > 29 => return,
        3 => (AskKind::Set, true),
        4 if r.vanabout_slot(n) => (AskKind::Remove, true),
        _ => return,
    };
    if l.mode != 2 && (o.is_none() || r.done(id, o.as_deref())) {
        return;
    }
    w.confirm = Some((super::Confirm::Roe(Ask { kind, n }), yes));
}

/// FUN_10098fe0: c2s 0x10C / 0x10D / 0x10E {u32 n}; the cooldown starts again, and a description goes back to the
/// list (not after a set).
fn send(w: &mut World, id: u16, n: u16) {
    log::info!("RoE: c2s {id:#x} objective {n}");
    let _ = w.net.commands.send(NetCommand::Packet { id, body: (n as u32).to_le_bytes().to_vec() });
    if let Some(l) = w.quests.as_mut().and_then(|q| q.roe.as_mut()) {
        l.sent = Some(Instant::now());
        if id != 0x10C && l.state == 1 {
            l.state = 0;
        }
    }
}

pub fn ask_text(gu: &GameUi, a: Ask) -> String {
    let id = match a.kind {
        AskKind::Set => 0x6f,
        AskKind::Remove => 0x73,
        AskKind::Reset => 0x74,
        AskKind::Claim => 0x75,
    };
    gu.lobby_msg(0xc, id, "")
}

/// The prompts' Yes (LAB_10232ab0, LAB_10232ae0, LAB_10232970, LAB_10232a40), each behind the cooldown.
pub fn answer(w: &mut World, a: Ask) {
    if !w.quests.as_ref().and_then(|q| q.roe.as_ref()).is_some_and(ready) {
        return;
    }
    let id = a.n.wrapping_add(0xe000);
    let o = objective(w, id);
    match a.kind {
        AskKind::Set => send(w, 0x10C, a.n),
        AskKind::Claim => send(w, 0x10E, a.n),
        // Vana'Bout's hold prompt (0xc 0x20d, "menu    holdtime") is not done: LSB never fills those slots.
        AskKind::Remove if w.roe.vanabout_slot(a.n) => log::info!("RoE: the Vana'Bout hold prompt is not done"),
        AskKind::Remove if w.roe.progress(id) > 0 => w.confirm = Some((super::Confirm::Roe(Ask { kind: AskKind::Reset, n: a.n }), false)),
        AskKind::Remove | AskKind::Reset if !w.roe.claimable(id, o.as_deref()) => send(w, 0x10D, a.n),
        _ => {}
    }
}

/// The prompts but the second one open under quest01 (FUN_10119110: its own x, quest01's bottom + 2).
pub fn under_list(a: Ask) -> bool {
    a.kind != AskKind::Reset
}

/// FUN_10232b80's keys.
pub fn key(w: &mut World, l: &mut List, key: egui::Key, page: bool) {
    use egui::Key as K;
    let n = l.rows.len();
    match (l.state, key) {
        (0, K::Escape) if l.stack.len() > 1 => {
            w.sounds.push(Ui::Cancel as u32);
            let (_, c) = l.stack.pop().unwrap();
            l.built = false;
            build_tree(w, l);
            l.cursor = c;
            show(l);
        }
        (0, K::Escape) => {
            w.sounds.push(Ui::Cancel as u32);
            if let Some(q) = w.quests.as_mut() {
                q.roe = None;
            }
        }
        (0, K::ArrowUp | K::ArrowDown) if n > 0 => {
            l.cursor = if key == K::ArrowDown { (l.cursor + 1) % n } else { (l.cursor + n - 1) % n };
            show(l);
            w.sounds.push(Ui::Cursor as u32);
        }
        (0, K::ArrowLeft | K::ArrowRight) if page => {
            let m = if key == K::ArrowRight { super::scrollbar::Msg::PageDown } else { super::scrollbar::Msg::PageUp };
            let mut row = l.cursor.saturating_sub(l.top);
            if super::scrollbar::paged(m, &mut l.top, &mut row, n, VISIBLE) {
                l.cursor = l.top + row;
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        // FUN_102304b0 / FUN_102304e0: the sibling category in its place.
        (0, K::ArrowLeft | K::ArrowRight) if l.mode == 3 && l.stack.len() > 1 => {
            let to = if key == K::ArrowRight { l.siblings.0 } else { l.siblings.1 };
            if to != 0 {
                l.stack.last_mut().unwrap().0 = to;
                l.built = false;
                build_tree(w, l);
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        (0, K::Enter) => {
            w.sounds.push(Ui::Select as u32);
            let Some(&id) = l.rows.get(l.cursor) else { return };
            match l.mode {
                2 if (0xe000..0xf200).contains(&id) => describe(w, l, id),
                // FUN_10230450: a category goes on the stack (5 deep), its rows from row 0.
                3 | 4 if id >= 0xf200 && id != 0xffff => {
                    if l.stack.len() < 5 {
                        if let Some(s) = l.stack.last_mut() {
                            s.1 = l.cursor;
                        }
                        l.stack.push((id, 0));
                        l.built = false;
                        build_tree(w, l);
                    }
                }
                3 | 4 => describe(w, l, id),
                _ => {}
            }
        }
        (1, K::Enter) => {
            w.sounds.push(Ui::Select as u32);
            if l.described < 0xf200 {
                prompt(w, l);
            }
        }
        (1, K::Escape) => {
            w.sounds.push(Ui::Cancel as u32);
            l.state = 0;
        }
        _ => {}
    }
}

/// Key function 0x18 (Num-) or a middle click: the description's other page.
pub fn flip_page(w: &mut World) {
    if let Some(l) = w.quests.as_mut().and_then(|q| q.roe.as_mut()).filter(|l| l.state == 1) {
        l.page2 = !l.page2;
    }
}

pub(super) fn bar(w: &mut World, m: super::scrollbar::Msg) -> bool {
    let Some(l) = w.quests.as_mut().and_then(|q| q.roe.as_mut()).filter(|l| l.state == 0) else { return false };
    let mut row = l.cursor.saturating_sub(l.top);
    let moved = super::scrollbar::paged(m, &mut l.top, &mut row, l.rows.len(), VISIBLE);
    l.cursor = l.top + row;
    moved
}

/// The arrows (children 9 / 10, FUN_102324f0) work only with both siblings.
fn arrows(l: &List) -> bool {
    l.mode == 3 && l.stack.len() > 1 && l.siblings.0 != 0 && l.siblings.1 != 0
}

/// Mouse: rows hover and click as Enter, the arrows as Left / Right, a click in a description as Enter.
pub fn pointer(w: &mut World, l: &List, hit: Option<u16>, moved: bool, clicked: bool) {
    let Some(c) = hit else { return };
    let mut l = l.clone();
    match c {
        9 | 10 if clicked && l.state == 0 && arrows(&l) => key(w, &mut l, if c == 9 { egui::Key::ArrowLeft } else { egui::Key::ArrowRight }, false),
        1..=8 if l.state == 0 => {
            let row = l.top + c as usize - 1;
            if row >= l.rows.len() {
                return;
            }
            if row != l.cursor && (moved || clicked) {
                l.cursor = row;
                w.sounds.push(Ui::Cursor as u32);
            } else if clicked {
                key(w, &mut l, egui::Key::Enter, false);
            }
        }
        _ if clicked && l.state == 1 => key(w, &mut l, egui::Key::Enter, false),
        _ => return,
    }
    if let Some(q) = w.quests.as_mut()
        && q.roe.is_some()
    {
        q.roe = Some(l);
    }
}

/// FUN_10231140: the row and description colour.
fn colour(w: &World, mode: usize, id: u16, o: Option<&Objective>, in_list: bool) -> u32 {
    let r = &w.roe;
    if mode == 4 && r.done(id, o) {
        return if o.map_or(0, |o| o.goal) != r.progress(id) { 0x8080_4040 } else { GREY };
    }
    if r.claimable(id, o) {
        0x8090_9040
    } else if r.timed(id) {
        0x8050_50a0
    } else if r.is_active(id) {
        0x8050_9050
    } else if (r.full() && in_list) || r.done(id, o) {
        GREY
    } else {
        WHITE
    }
}

/// FUN_10233280's kind: the yellow "new" bubble (keytops3 170) on an objective never completed and not claimable,
/// else framesus 35.
fn icon(gu: &mut GameUi, w: &World, id: u16, o: Option<&Objective>, x: f32, y: f32) {
    if id < 0xf200 && !w.roe.completed_id(id) && !w.roe.claimable(id, o) {
        gu.sprite("menu    keytops3", 170, x, y + 1.0, WHITE);
    } else {
        gu.sprite("menu    framesus", 35, x + 4.0, y + 5.0, WHITE);
    }
}

/// The count shown: a record done for good and no longer taken shows its goal.
fn count(w: &World, id: u16, o: &Objective) -> (u32, bool) {
    let r = &w.roe;
    if !r.claimable(id, Some(o)) && r.done(id, Some(o)) && !r.is_active(id) { (o.goal, true) } else { (r.progress(id), false) }
}

fn text(gu: &mut GameUi, s: &str, x: f32, y: f32, tint: u32) {
    gu.text(s, x, y, 0, tint.to_le_bytes());
}

/// quest01 in the RoE modes (0x10232530): the title and empty texts, the rows (icon, name, gauge and count), the
/// description with its count and rewards (FUN_102323d0, FUN_10232090), the arrows.
pub fn draw(gu: &mut GameUi, w: &World, l: &List, sw: f32, sh: f32, rects: &mut Vec<[f32; 4]>) {
    let shown: Vec<u16> = if l.state == 0 { (1..=VISIBLE as u16).filter(|&c| l.top + (c as usize) - 1 < l.rows.len()).collect() } else { Vec::new() };
    let off: Vec<u16> = if arrows(l) { Vec::new() } else { vec![9, 10] };
    let hide: Vec<u16> = (1..=VISIBLE as u16).filter(|c| !shown.contains(c)).chain(off.iter().copied()).collect();
    let sel = (l.state == 0 && !l.rows.is_empty()).then(|| (l.cursor - l.top + 1) as u16);
    let Some(win) = gu.lobby_window_off(LIST, sw, sh, (0.0, 0.0), sel, w.confirm.is_none(), &hide) else { return };
    let [fx, fy, fw, fh] = win.frame;
    let Some((x, y)) = gu.window_xy(LIST, sw, sh) else { return };
    rects.push(win.frame);
    gu.disabled_children(LIST, x, y, &off, None);
    if l.state == 0 {
        let title = if l.stack.len() > 1 {
            w.item_info.as_ref().and_then(|c| c.category(l.stack[l.stack.len() - 1].0)).map(|c| c.name.clone()).unwrap_or_default()
        } else {
            match l.mode {
                2 => format!("{}          {}/30", gu.lobby_msg(0xc, 0x6d, ""), w.roe.taken()),
                3 => gu.lobby_msg(0xc, 0x6e, ""),
                _ => gu.lobby_msg(0xc, 0x208, ""),
            }
        };
        text(gu, &title, x + 60.0, y + 10.0, WHITE);
        if l.built && l.rows.is_empty() && l.mode != 4 {
            text(gu, &gu.lobby_msg(0xc, if l.mode == 2 { 0x72 } else { 0x87 }, ""), x + 20.0, y + 40.0, WHITE);
        }
        for &(c, [cx, cy, ..]) in win.children.iter().filter(|(id, _)| (1..=VISIBLE as u16).contains(id)) {
            let row = l.top + c as usize - 1;
            let Some(&id) = l.rows.get(row) else { continue };
            if id >= 0xf200 {
                let name = w.item_info.as_ref().and_then(|i| i.category(id)).map(|c| c.name.clone()).unwrap_or_default();
                icon(gu, w, id, None, cx, cy);
                super::missions::lines(gu, &name, cx + 18.0, cy, 301.0, if row == l.cursor { CURSOR } else { WHITE });
                continue;
            }
            let o = objective(w, id);
            icon(gu, w, id, o.as_deref(), cx, cy);
            let tint = if row == l.cursor { CURSOR } else { colour(w, l.mode, id, o.as_deref(), l.mode == 3) };
            super::missions::lines(gu, o.as_ref().map_or("", |o| &o.name), cx + 18.0, cy, 301.0, tint);
            // FUN_10231300: the gauge (windowps 97 scaled by the count, frame 38) and "%d/%d" in fontshp.
            if let Some(o) = &o {
                let (have, _) = count(w, id, o);
                let f = if o.goal == 0 { 1.0 } else { (have as f32 / o.goal as f32).min(1.0) };
                gu.sprite_scaled("menu    windowps", 97, cx + 249.0, cy + 18.0, f, 1.0, WHITE);
                gu.sprite("menu    windowps", 38, cx + 245.0, cy + 18.0, WHITE);
                gu.shape_text(&format!("{have}/{}", o.goal), cx + 312.0, cy + 15.0, WHITE, true);
            }
        }
        let bx = fx + fw - 8.0;
        gu.scroll_bar(bx, fy, fh, l.rows.len(), VISIBLE, l.top);
        gu.bar_site(super::scrollbar::Owner::Journal, bx, fy, fh, l.rows.len(), VISIBLE, l.top);
        return;
    }
    let id = l.described;
    let Some(o) = objective(w, id) else { return };
    if id < 0xf200 && !w.roe.completed_id(id) && !w.roe.claimable(id, Some(&o)) {
        gu.sprite("menu    keytops3", 170, x + 32.0, y + 21.0, WHITE);
    } else {
        gu.sprite("menu    framesus", 35, x + 36.0, y + 25.0, WHITE);
    }
    text(gu, &o.name, x + 50.0, y + 20.0, colour(w, l.mode, id, Some(&o), false));
    let desc = if o.pages[1].is_empty() { o.pages[0].clone() } else { format!("{} ▼", o.pages[l.page2 as usize]) };
    super::missions::lines(gu, &desc, x + 40.0, y + 50.0, 327.0, WHITE);
    // FUN_10232090.
    let (have, full) = count(w, id, &o);
    let at_goal = l.mode == 4 && w.roe.done(id, Some(&o)) && have == o.goal;
    text(gu, &format!("{}：", gu.lobby_msg(0xc, 0x81, "")), x + 20.0, y + 180.0, WHITE);
    text(gu, &format!("{have}/{}", o.goal), x + 180.0, y + 180.0, if full || at_goal { 0x8020_60a0 } else { WHITE });
    if !o.repeatable {
        text(gu, &gu.lobby_msg(0xc, 0x84, ""), x + 220.0, y + 200.0, 0x8080_4040);
    }
    text(gu, &gu.lobby_msg(0xc, 0x83, ""), x + 20.0, y + 200.0, WHITE);
    let extra = |v: u32, m: usize| (v != 0).then(|| format!("{v}{}", gu.lobby_msg(0xc, m, "")));
    let tail: Vec<String> = [extra(o.cap, 0x7d), extra(o.acc, 0x7e)].into_iter().flatten().collect();
    let line = format!(
        "{}{}  {}{}  {}  {}",
        o.sparks,
        gu.lobby_msg(0xc, 0x7b, ""),
        o.exp,
        gu.lobby_msg(0xc, 0x7c, ""),
        tail.first().map_or("", |s| s.as_str()),
        tail.get(1).map_or("", |s| s.as_str())
    );
    text(gu, &line, x + 20.0, y + 220.0, WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roe() -> Roe {
        Roe { flags: 7, ..Default::default() }
    }

    #[test]
    fn conditions() {
        let mut r = roe();
        r.log[0] = 0b10;
        r.rot[1] = 3;
        let mut zones = [0u8; 0x30];
        zones[261 >> 3] |= 1 << (261 & 7);
        let mut skills = [0u16; 64];
        skills[1] = 0x8000;
        skills[2] = 40;
        let mut levels = [0u8; 24];
        (levels[1], levels[3]) = (30, 5);
        let c = Ctx { roe: &r, levels, zones: Some(&zones), skills: Some(&skills), unity: 4 };
        let one = |b: [u8; 4]| passes(&c, &[b, [0; 4], [0; 4], [0; 4]]);
        assert!(one([1, 0, 0, 1]) && !one([2, 0, 0, 1]));
        assert!(one([0b101, 0, 0, 2]) && !one([0b110, 0, 0, 2]));
        assert!(one([30, 1, 0, 3]) && !one([31, 1, 0, 3]));
        assert!(one([5, 1, 0, 4]) && !one([6, 1, 0, 4]));
        assert!(one([40, 0, 2, 5]) && !one([41, 0, 2, 5]) && !one([0, 0, 1, 5]));
        assert!(one([3, 1, 0, 6]) && !one([3, 0, 0, 6]));
        assert!(one([4, 0, 0, 7]) && !one([0, 0, 0, 7]));
        assert!(one([1, 0, 0, 8]) && !one([0, 0, 0, 8]));
        assert!(!one([0, 0, 0, 9]));
    }

    #[test]
    fn predicates_and_siblings() {
        let mut r = roe();
        r.active[0] = 3 << 12 | 0x0c;
        r.active[63] = 0x5;
        let goal = |g| Objective { goal: g, ..Default::default() };
        assert!(r.is_active(0xe00c) && !r.is_active(0xe00d));
        assert_eq!(r.progress(0xe00c), 3);
        assert!(r.claimable(0xe00c, Some(&goal(3))) && !r.claimable(0xe00c, Some(&goal(4))));
        assert!(r.timed(0xe005) && !r.timed(0xe006) && !r.full());
        assert_eq!(r.slots(), 1);
        let c = Ctx { roe: &r, levels: [0; 24], zones: None, skills: None, unity: 0 };
        let parent = Category { name: String::new(), kids: vec![(0xf210, [[0; 4]; 4]), (0xf211, [[9, 0, 0, 1], [0; 4], [0; 4], [0; 4]]), (0xf212, [[0; 4]; 4])] };
        // 0xf211 is hidden (objective 9 not completed): both sides skip it.
        assert_eq!(siblings(&c, &parent, 0xf210), (0xf212, 0xf212));
        assert_eq!(siblings(&c, &parent, 0xf212), (0xf210, 0xf210));
        assert_eq!(visible(&c, &parent), vec![0xf210, 0xf212]);
    }
}
