//! Training regimes (an Extra, not retail: retail has no tracker; the user's wish, 2026-10-28; spec
//! ~/ffxi/lab/research/training_regime_spec.md). The Fields / Grounds of Valor page in force and its kills, in a window
//! docked with the Timers that slides in and out. Learnt passively, nothing is sent for it: the Field Manual / Grounds
//! Tome event's own traffic (its start's num[7], the page answer s2c 0x05C, our end option, the 0x02A confirmation, the
//! Review answer), the kill credits (0x029 558), completion (559) and repeats (643); the labels from the zone
//! dialogue's regime block. The server re-sends nothing at login, so the state is kept per character on disk; kills
//! seen with no page known name their rows after the monsters until a book is visited.

use super::World;
use super::game_ui::GameUi;
use super::net::{BattleMessage, EventStart};
use super::timers::{Dock, Place};
use crate::dat::dialog::Dialog;
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// 0x029: a defeat (6 "defeats", 20 "falls to the ground": the target is the monster), a kill credit (558: Data killed,
/// Data2 needed), the page done (559), a repeat begun anew (643).
const DEFEATS: [u16; 2] = [6, 20];
const CREDIT: u16 = 558;
const DONE: u16 = 559;
const ANEW: u16 = 643;
/// How long after a book event's end its confirmation may come, and a defeat still names a credit's monster *(inf: the
/// server sends both in one tick)*.
const CONFIRM_SECS: f32 = 5.0;
const DEFEAT_SECS: f32 = 2.0;

/// Fields of Valor 1..146, Grounds of Valor 602..819 (147..559 are hunts).
pub fn is_regime(id: u32) -> bool {
    matches!(id, 1..=146 | 602..=819)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Slot {
    pub label: String,
    /// 0: not known yet.
    pub needed: u8,
    pub killed: u8,
    /// The killed count is the server's (a credit or a Review), not a guess.
    pub known: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Regime {
    /// 0: learnt from kills alone (no book seen), the rows named after the monsters.
    pub id: u16,
    /// "Garlaige Citadel" (the R + 3 + id text).
    pub area: String,
    pub slots: Vec<Slot>,
    pub repeat: bool,
    /// 559 came (hidden until 643 begins it anew or a book says otherwise).
    pub complete: bool,
}

impl Regime {
    pub fn shown(&self) -> bool {
        !self.complete && !self.slots.is_empty()
    }

    pub fn heading(&self) -> String {
        if self.area.is_empty() { "Training Regime".into() } else { self.area.clone() }
    }
}

/// A row as shown: "Siege Bat: 3/4", done, and how full.
pub fn row_text(s: &Slot) -> (String, bool, Option<f32>) {
    let n = if s.needed == 0 { "?".to_string() } else { s.needed.to_string() };
    let k = if s.known { s.killed.to_string() } else { "?".into() };
    let done = s.known && s.needed > 0 && s.killed >= s.needed;
    let f = (s.known && s.needed > 0).then(|| s.killed as f32 / s.needed as f32);
    (format!("{}: {k}/{n}", s.label), done, f)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Pending {
    Accept { repeat: bool },
    Cancel,
}

/// A Field Manual / Grounds Tome (or Hunt Registry) event in progress.
struct Book {
    hunt: bool,
    /// The start's num[7]: the server's regime id (0 none, a hunt's id).
    start: u32,
    /// Our last update's option and the page answer to option 2 | page << 4.
    option: u32,
    page: Option<[u32; 8]>,
    pending: Option<(Pending, Instant)>,
}

pub struct Tracker {
    pub state: Option<Regime>,
    book: Option<Book>,
    defeat: Option<(String, Instant)>,
    /// The character the state belongs to (loaded at its first zone-in).
    who: String,
    /// The zone's regime block base R (its "New training regime registered!" entry).
    r: Option<(u16, u32)>,
    /// The drawer is out; `slide` runs 0 (in) .. 1 (out); `dirty` when it changed (the app keeps it in the config).
    pub out: bool,
    slide: f32,
    pub dirty: bool,
    last: Option<Instant>,
    frame: Option<(Regime, Place)>,
    rect: Option<[f32; 4]>,
    drawer: Option<[f32; 4]>,
}

impl Tracker {
    pub fn new(out: bool) -> Self {
        Self { state: None, book: None, defeat: None, who: String::new(), r: None, out, slide: out as u8 as f32, dirty: false, last: None, frame: None, rect: None, drawer: None }
    }

    pub fn toggle(&mut self) {
        self.out = !self.out;
        self.dirty = true;
    }

    /// The regime to show, if any.
    pub fn shown(&self) -> Option<&Regime> {
        self.state.as_ref().filter(|r| r.shown())
    }
}

// ---- The book's labels ----

/// One page line's label: "[Siege Bat/Siege Bats]" its singular, "member[/s] of the worm family" "Worm family",
/// "undead-type creature[/s]" "Undead-type".
pub fn label(line: &[u8]) -> String {
    let mut out = Vec::new();
    let mut i = 0;
    while i < line.len() {
        match line[i] {
            0x0a => i += 2,
            0x7f => i += if line.get(i + 1) == Some(&0x92) { 3 } else { 2 },
            b'[' => {
                let end = line[i..].iter().position(|&b| b == b']').map_or(line.len(), |p| i + p);
                let inner = &line[i + 1..end];
                out.extend_from_slice(inner.split(|&b| b == b'/').next().unwrap_or_default());
                i = end + 1;
            }
            c if c >= 0x80 => {
                out.extend_from_slice(&line[i..(i + 2).min(line.len())]);
                i += 2;
            }
            c if c < 0x20 => i += 1,
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    let s = crate::dat::font::decode_sjis(&out);
    let s = s.trim().trim_end_matches('.').trim();
    let cap = |t: &str| {
        let mut c = t.chars();
        c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
    };
    if let Some(f) = s.strip_prefix("member of the ").and_then(|r| r.strip_suffix(" family")) {
        return format!("{} family", cap(f));
    }
    if let Some(k) = s.strip_suffix(" creature") {
        return cap(k);
    }
    cap(s)
}

/// A text's lines (split at 07, up to the 00 that ends it; code arguments stepped over: 0a n, 7f 92 n, 7f x).
fn lines(raw: &[u8]) -> Vec<&[u8]> {
    let (mut out, mut start, mut i) = (Vec::new(), 0, 0);
    while i < raw.len() {
        match raw[i] {
            0x00 => break,
            0x07 => {
                out.push(&raw[start..i]);
                start = i + 1;
                i += 1;
            }
            0x0a => i += 2,
            0x7f => i += if raw.get(i + 1) == Some(&0x92) { 3 } else { 2 },
            c if c >= 0x80 => i += 2,
            _ => i += 1,
        }
    }
    out.push(&raw[start..i.min(raw.len())]);
    out
}

/// The page's labels (R - 2050 + id: a line each, every one with its count code) and the area (R + 3 + id).
pub fn page(d: &Dialog, r: u32, id: u16) -> Option<(Vec<String>, String)> {
    let raw = d.raw((r + id as u32).checked_sub(2050)?)?;
    let lines: Vec<&[u8]> = lines(&raw).into_iter().filter(|l| l.iter().any(|&b| b > b' ')).collect();
    if lines.is_empty() || lines.len() > 4 || !lines.iter().all(|l| l.contains(&0x0a)) {
        return None;
    }
    let area = d.raw(r + 3 + id as u32).map(|a| {
        let a = &a[..a.iter().position(|&b| b == 0 || b == 0x7f).unwrap_or(a.len())];
        let t = crate::dat::font::decode_sjis(a);
        t.trim().trim_start_matches("Training area:").trim().trim_end_matches('.').to_string()
    });
    Some((lines.iter().map(|l| label(l)).collect(), area.unwrap_or_default()))
}

/// The block's base: the entry that is "New training regime registered!" (one per book zone).
fn find_r(d: &Dialog) -> Option<u32> {
    (0..d.len() as u32).find(|&i| d.raw(i).is_some_and(|r| r.starts_with(b"New training regime registered!")))
}

/// This zone's dialogue and its block base (looked up once a zone).
fn zone_block(w: &mut World) -> Option<(Dialog, u32)> {
    let zone = w.zone?;
    let index = w.index.clone()?;
    let d = Dialog::load(&index, zone).ok()?;
    let r = match w.regime.r {
        Some((z, r)) if z == zone => r,
        _ => {
            let r = find_r(&d)?;
            w.regime.r = Some((zone, r));
            r
        }
    };
    Some((d, r))
}

// ---- Keeping it ----

fn save(w: &World) {
    if w.regime.who.is_empty() {
        return;
    }
    let path = crate::config::regime_path(&w.regime.who);
    let r = std::fs::create_dir_all(path.parent().unwrap_or(&path)).map_err(anyhow::Error::from).and_then(|_| Ok(std::fs::write(&path, serde_json::to_vec_pretty(&w.regime.state)?)?));
    if let Err(e) = r {
        log::warn!("regime: {}: {e:#}", path.display());
    }
}

fn set(w: &mut World, state: Option<Regime>, why: &str) {
    log::info!("regime: {why}: {state:?}");
    w.regime.state = state;
    save(w);
}

/// At each zone-in: the character's kept state, once.
pub fn on_zone(w: &mut World) {
    w.regime.book = None;
    if w.regime.who == w.name || w.name.is_empty() {
        return;
    }
    w.regime.who = w.name.clone();
    let path = crate::config::regime_path(&w.name);
    w.regime.state = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Option<Regime>>(&b).ok()).flatten();
    if let Some(r) = &w.regime.state {
        log::info!("regime: kept for {}: {r:?}", w.name);
    }
}

// ---- What the server tells us ----

/// s2c 0x034 / 0x032: a Field Manual or Grounds Tome tells the server's regime (num[7]); a Hunt Registry may cancel it.
pub fn on_event_start(w: &mut World, s: &EventStart) {
    let name = w.entities.get(&s.target).map(|e| e.name.clone()).unwrap_or_default();
    let (book, hunt) = (matches!(name.as_str(), "Field Manual" | "Grounds Tome"), name == "Hunt Registry");
    w.regime.book = (book || hunt).then_some(Book { hunt, start: s.params[7], option: 0, page: None, pending: None });
    if !book {
        return;
    }
    let id = s.params[7];
    if !is_regime(id) {
        if w.regime.state.is_some() {
            set(w, None, "the book says none");
        }
        return;
    }
    if w.regime.state.as_ref().is_some_and(|r| r.id as u32 == id) {
        return;
    }
    // A page we did not see accepted: its labels now, the counts from kills or a Review; a kills-only state's counts go
    // to the slots their monsters fit.
    let Some((d, r)) = zone_block(w) else { return };
    let Some((labels, area)) = page(&d, r, id as u16) else { return };
    let old = w.regime.state.take().filter(|o| o.id == 0);
    let mut slots: Vec<Slot> = labels.into_iter().map(|label| Slot { label, ..Default::default() }).collect();
    for o in old.iter().flat_map(|o| &o.slots) {
        if let Some(s) = slots.iter_mut().find(|s| !s.known && matches(&s.label, &o.label)) {
            (s.needed, s.killed, s.known) = (o.needed, o.killed, o.known);
        }
    }
    set(w, Some(Regime { id: id as u16, area, slots, repeat: false, complete: false }), "the book's regime");
}

/// Our c2s 0x05B at a book: an update's option, or the end (2 | page << 4, bit 31 repeat: an accept; 3 a cancel; at a
/// Hunt Registry 4 cancels).
pub fn on_send(w: &mut World, update: bool, option: u32) {
    let Some(b) = w.regime.book.as_mut() else { return };
    if update {
        b.option = option;
        return;
    }
    let p = if b.hunt {
        (option == 4).then_some(Pending::Cancel)
    } else if option & 0xf == 2 && (option >> 4) & 0xff >= 1 {
        Some(Pending::Accept { repeat: option >> 31 != 0 })
    } else {
        (option == 3).then_some(Pending::Cancel)
    };
    b.pending = p.map(|p| (p, Instant::now()));
}

/// s2c 0x05C at a book: the page shown (num[0..3] needed, num[7] its id) or the Review (num[0..3] needed, num[4..7]
/// killed).
pub fn on_params(w: &mut World, num: [u32; 8]) {
    let Some(b) = w.regime.book.as_mut() else { return };
    match b.option {
        1 if num[0] != 0 && is_regime(b.start) => {
            let id = b.start as u16;
            let Some((d, r)) = zone_block(w) else { return };
            let Some((labels, area)) = page(&d, r, id) else { return };
            let slots = labels.into_iter().enumerate().map(|(k, label)| Slot { label, needed: num[k] as u8, killed: num[k + 4] as u8, known: true }).collect();
            let repeat = w.regime.state.as_ref().is_some_and(|s| s.id == id && s.repeat);
            set(w, Some(Regime { id, area, slots, repeat, complete: false }), "Review");
        }
        o if o & 0xf == 2 => b.page = Some(num),
        _ => {}
    }
}

/// s2c 0x02A (installed id `id`, MesNum bit 15 `plain`, about `actor`): after our accept, the block's "New training
/// regime registered!" (its id is R; the page must fit the block, else it was a refusal); after a cancel, any line.
pub fn on_zone_text(w: &mut World, id: u32, plain: bool, actor: u32) {
    let me = w.my_id;
    let Some(b) = w.regime.book.as_mut() else { return };
    let Some((p, at)) = b.pending else { return };
    if at.elapsed().as_secs_f32() > CONFIRM_SECS {
        b.pending = None;
        return;
    }
    if !plain || actor != me {
        return;
    }
    b.pending = None;
    match p {
        Pending::Cancel => set(w, None, "cancelled"),
        Pending::Accept { repeat } => {
            let Some(num) = b.page else { return };
            let rid = num[7];
            let Some(index) = w.index.clone() else { return };
            let Some(d) = w.zone.and_then(|z| Dialog::load(&index, z).ok()) else { return };
            let needed: Vec<u8> = num[..4].iter().map(|&n| n as u8).take_while(|&n| n > 0).collect();
            match page(&d, id, rid as u16).filter(|(l, _)| is_regime(rid) && l.len() == needed.len()) {
                Some((labels, area)) => {
                    if let Some(z) = w.zone {
                        w.regime.r = Some((z, id));
                    }
                    let slots = labels.into_iter().zip(needed).map(|(label, needed)| Slot { label, needed, killed: 0, known: true }).collect();
                    set(w, Some(Regime { id: rid as u16, area, slots, repeat, complete: false }), "accepted");
                }
                None => log::info!("regime: text {id} after an accept is not the block's (a refusal?)"),
            }
        }
    }
}

/// A label fits a monster's name: the same name, or a family / type word starting one of its words ("Bat family"
/// fits "Seeker Bats").
pub fn matches(label: &str, name: &str) -> bool {
    if label.eq_ignore_ascii_case(name) {
        return true;
    }
    let word = label.strip_suffix(" family").or_else(|| label.strip_suffix("-type"));
    word.is_some_and(|f| name.split(|c: char| !c.is_alphanumeric()).any(|w| w.to_lowercase().starts_with(&f.to_lowercase())))
}

fn kind_slot(label: &str) -> bool {
    label.ends_with(" family") || label.ends_with("-type")
}

/// Which slot a credit (k of n) is (spec 7.3): the ones it fits by the counts, then the defeated monster's name, then
/// the lowest; with none, the slot of that count nearest. None when nothing takes it.
pub fn attribute(slots: &[Slot], k: u8, n: u8, name: Option<&str>) -> Option<usize> {
    let fits = |s: &Slot| (s.needed == n || s.needed == 0) && (!s.known || s.killed + 1 == k);
    let c: Vec<usize> = (0..slots.len()).filter(|&i| fits(&slots[i])).collect();
    if c.len() == 1 {
        return Some(c[0]);
    }
    if c.len() > 1 {
        if let Some(name) = name {
            let m: Vec<usize> = c.iter().copied().filter(|&i| matches(&slots[i].label, name)).collect();
            if m.len() == 1 {
                return Some(m[0]);
            }
            let kinds: Vec<usize> = c.iter().copied().filter(|&i| kind_slot(&slots[i].label)).collect();
            if m.is_empty() && kinds.len() == 1 {
                return Some(kinds[0]);
            }
            if let Some(&i) = m.first() {
                return Some(i);
            }
        }
        // Counts that fit exactly before guesses.
        return c.iter().copied().find(|&i| slots[i].needed == n && slots[i].known).or(Some(c[0]));
    }
    (0..slots.len()).filter(|&i| slots[i].needed == n && slots[i].killed < n).min_by_key(|&i| (k as i32 - 1 - slots[i].killed as i32).abs())
}

/// s2c 0x029 / 0x02D.
pub fn on_battle(w: &mut World, m: &BattleMessage) {
    let me = w.my_id;
    let msg = m.message & 0x7fff;
    if DEFEATS.contains(&msg) {
        if let Some(e) = w.entities.get(&m.target) {
            w.regime.defeat = Some((e.name.clone(), Instant::now()));
        }
        return;
    }
    if m.target != me || !matches!(msg, CREDIT | DONE | ANEW) {
        return;
    }
    match msg {
        CREDIT => {
            let (k, n) = (m.data[0].clamp(0, 255) as u8, m.data[1].clamp(0, 255) as u8);
            let name = w.regime.defeat.take().filter(|(_, t)| t.elapsed().as_secs_f32() <= DEFEAT_SECS).map(|(n, _)| n);
            let mut st = w.regime.state.take().unwrap_or_default();
            st.complete = false;
            let i = match attribute(&st.slots, k, n, name.as_deref()) {
                Some(i) => i,
                // Kills alone: a row per monster, up to the page's four.
                None if st.id == 0 && st.slots.len() < 4 => {
                    st.slots.push(Slot { label: name.clone().unwrap_or_else(|| format!("Target {}", st.slots.len() + 1)), ..Default::default() });
                    st.slots.len() - 1
                }
                None => {
                    log::info!("regime: credit {k}/{n} ({name:?}) fits no slot of {:?}", st.slots);
                    w.regime.state = Some(st);
                    return;
                }
            };
            let s = &mut st.slots[i];
            (s.needed, s.killed, s.known) = (n, k, true);
            set(w, Some(st), &format!("credit {k}/{n} ({name:?})"));
        }
        DONE => {
            if let Some(mut st) = w.regime.state.take() {
                st.complete = true;
                set(w, Some(st), "complete");
            }
        }
        _ => {
            if let Some(mut st) = w.regime.state.take() {
                st.complete = false;
                st.repeat = true;
                for s in &mut st.slots {
                    (s.killed, s.known) = (0, true);
                }
                set(w, Some(st), "begun anew");
            }
        }
    }
}

// ---- The window (classic) ----

/// The window's width, inner margin and pitches (as the Timers').
const WIDTH: f32 = super::timers::WIDTH;
const PAD: f32 = 8.0;
const HEAD_H: f32 = 16.0;
const ROW_H: f32 = 24.0;
const SLIVER: f32 = 22.0;
const SLIDE_TICKS: f32 = 12.0;
const TOP_EDGE: f32 = 48.0;
const N: u32 = 0x8080_8080;
/// A full slot (the Records of Eminence list's done colour *(ours)*).
const DONE_TINT: u32 = 0x8040_4040;

fn ease(s: f32) -> f32 {
    s * s * (3.0 - 2.0 * s)
}

/// Where it goes: the Timers' dock, under the Timers window when that is up (above it on the right).
fn place_by(base: Place, timers: Option<[f32; 4]>) -> Place {
    match (base.dock, timers) {
        (Dock::Right, Some(r)) => {
            let y = r[1] - 4.0;
            Place { y, max_h: (base.max_h - (base.y - y)).max(0.0), ..base }
        }
        (_, Some(r)) => {
            let y = r[1] + r[3] + 4.0;
            Place { y, max_h: (base.max_h - (y - base.y)).max(0.0), ..base }
        }
        (_, None) => base,
    }
}

/// Draws the window at `p` with the drawer at `slide`; its rect and the drawer's.
pub fn draw_panel(gu: &mut GameUi, r: &Regime, p: &Place, slide: f32, sw: f32) -> Option<([f32; 4], [f32; 4])> {
    let rows = ((p.max_h - 2.0 * PAD - HEAD_H) / ROW_H).floor().max(0.0) as usize;
    let rows = rows.min(r.slots.len());
    if rows == 0 {
        return None;
    }
    let h = 2.0 * PAD + HEAD_H + ROW_H * rows as f32;
    let (mut x, mut y) = (p.x, if p.dock == Dock::Right { p.y - h } else { p.y });
    let e = 1.0 - ease(slide.clamp(0.0, 1.0));
    match p.dock {
        Dock::Left => x -= (x + WIDTH - SLIVER) * e,
        Dock::Right => x += (sw - SLIVER - x) * e,
        Dock::Top => y -= (y + h - (TOP_EDGE + SLIVER)) * e,
    }
    if p.dock == Dock::Top {
        gu.clip(Some([0.0, TOP_EDGE, sw, 1.0e5]));
    }
    gu.frame(x, y, WIDTH, h);
    let inward = slide < 0.5;
    match p.dock {
        Dock::Left => gu.sprite("menu    frames", if inward { 26 } else { 25 }, x + WIDTH - 20.0, y + 6.0, N),
        Dock::Right => gu.sprite("menu    frames", if inward { 25 } else { 26 }, x + 4.0, y + 6.0, N),
        Dock::Top => {
            let (m, dx, dy, ay) = if inward { ([0.0, 1.0, -1.0, 0.0], 0.0, 18.0, y + h - 18.0) } else { ([0.0, -1.0, 1.0, 0.0], 9.0, 0.0, y + 4.0) };
            gu.sprite_affine("menu    frames", 25, x + WIDTH - 18.0 + dx, ay + dy, m, N);
            true
        }
    };
    let drawer = match p.dock {
        Dock::Left => [x + WIDTH - SLIVER, y, SLIVER, h.min(48.0)],
        Dock::Right => [x, y, SLIVER, h.min(48.0)],
        Dock::Top if inward => [x, y + h - SLIVER, WIDTH, SLIVER],
        Dock::Top => [x + WIDTH - 24.0, y, 24.0, 24.0],
    };
    let rect = if p.dock == Dock::Top && y < TOP_EDGE { [x, TOP_EDGE, WIDTH, (y + h - TOP_EDGE).max(0.0)] } else { [x, y, WIDTH, h] };
    if slide <= 0.0 {
        gu.clip(None);
        return Some((rect, drawer));
    }
    let (rx, right, inner) = (x + PAD, x + WIDTH - PAD, WIDTH - 2.0 * PAD);
    let hx = if p.dock == Dock::Right { rx + 16.0 } else { rx };
    gu.shape_fit(&r.heading(), hx, y + PAD + 1.0, inner - 22.0, 0x80a0_a080);
    let mut ly = y + PAD + HEAD_H;
    for s in r.slots.iter().take(rows) {
        let (_, done, f) = row_text(s);
        let tint = if done { DONE_TINT } else { N };
        let k = if s.known { s.killed.to_string() } else { "?".into() };
        let n = if s.needed == 0 { "?".to_string() } else { s.needed.to_string() };
        gu.text_fit(&format!("{}:", s.label), rx, ly - 2.0, inner - 36.0, tint);
        gu.shape_text(&format!("{k}/{n}"), right, ly + 13.0, tint, true);
        let gy = ly + 14.0;
        if let Some(f) = f.filter(|f| *f > 0.0) {
            gu.sprite_coloured("menu    windowps", 97, rx + 4.0, gy + 1.0, f.min(1.0), 1.0, if done { [0x40, 0x40, 0x40, 0xa9] } else { [0x20, 0x70, 0x80, 0xa9] }, 8);
        }
        gu.sprite("menu    windowps", 38, rx, gy, if f.is_some_and(|f| f > 0.0) { N } else { 0x3050_5050 });
        ly += ROW_H;
    }
    gu.clip(None);
    Some((rect, drawer))
}

/// Each frame from the HUD after the Timers' prepare: hidden in events, under the map, and with nothing to show.
pub fn prepare(gu: &GameUi, w: &mut World, base: Place) {
    let now = Instant::now();
    let dt = w.regime.last.map_or(0.0, |l| now.duration_since(l).as_secs_f32());
    w.regime.last = Some(now);
    let step = dt * 60.0 / SLIDE_TICKS;
    w.regime.slide = if w.regime.out { (w.regime.slide + step).min(1.0) } else { (w.regime.slide - step).max(0.0) };
    w.regime.frame = None;
    let _ = gu;
    if !w.extra_regime || super::event::active(w) || w.map.is_some() || w.cnq.is_some() {
        return;
    }
    let Some(r) = w.regime.shown().cloned() else { return };
    let timers = if w.extra_timers { w.timers.rect() } else { None };
    w.regime.frame = Some((r, place_by(base, timers)));
}

pub fn draw(gu: &mut GameUi, t: &mut Tracker, sw: f32) {
    (t.rect, t.drawer) = (None, None);
    let Some((r, p)) = t.frame.take() else { return };
    if let Some((rect, drawer)) = draw_panel(gu, &r, &p, t.slide, sw) {
        (t.rect, t.drawer) = (Some(rect), Some(drawer));
    }
}

/// After every window: a click on the drawer's arrow, unless a window over ours has the spot. The window's rect for
/// the world-click blockers.
pub fn pointer(w: &mut World, gu: &GameUi, p: Option<(f32, f32)>, clicked: bool, over: &[[f32; 4]]) -> Option<[f32; 4]> {
    let r = w.regime.rect?;
    let Some(p) = p else { return Some(r) };
    let inside = |r: &[f32; 4]| p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3];
    if !inside(&r) || over.iter().any(inside) || !w.regime.drawer.as_ref().is_some_and(inside) {
        return Some(r);
    }
    gu.hover.set(true);
    if clicked {
        w.regime.toggle();
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    Some(r)
}

/// topbar-shot's sample (`--window regime:DOCK[:SLIDE]`): Garlaige's page 704 half done.
pub fn shot(gu: &mut GameUi, spec: &str, sw: f32, sh: f32, mini: Option<[f32; 4]>) {
    let f: Vec<&str> = spec.split(':').collect();
    let dock = Dock::of(f.first().and_then(|d| d.parse().ok()).unwrap_or(0));
    let slide = f.get(1).and_then(|d| d.parse().ok()).unwrap_or(1.0);
    let r = Regime {
        id: 704,
        area: "Garlaige Citadel".into(),
        slots: vec![Slot { label: "Siege Bat".into(), needed: 4, killed: 3, known: true }, Slot { label: "Borer Beetle".into(), needed: 2, killed: 2, known: true }],
        repeat: false,
        complete: false,
    };
    let log_top = sh - 16.0 - 8.0 * 16.0 - 14.0;
    let foot = sh - 16.0 - 34.0 - 46.0;
    let p = super::timers::place(dock, sw, sh, 4, log_top, foot, mini);
    draw_panel(gu, &r, &p, slide, sw);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(label: &str, needed: u8, killed: u8, known: bool) -> Slot {
        Slot { label: label.into(), needed, killed, known }
    }

    /// The page lines' three forms (spec 2.2).
    #[test]
    fn labels() {
        assert_eq!(label(b"\x0a\x00 \x7f\x92\x00[Siege Bat/Siege Bats]."), "Siege Bat");
        assert_eq!(label(b"\x0a\x01 member\x7f\x92\x01[/s] of the worm family."), "Worm family");
        assert_eq!(label(b"\x0a\x02 undead-type creature\x7f\x92\x02[/s]."), "Undead-type");
    }

    /// Zone 100's block on the real dialogue: R = the "registered" entry; page 2 (Forest Hares) and Garlaige's 704.
    #[test]
    fn real_block() {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let Ok(d) = Dialog::load(&index, 100) else { return };
        let Some(r) = find_r(&d) else { panic!("no block in zone 100") };
        let (l, area) = page(&d, r, 2).unwrap();
        assert_eq!(l, ["Forest Hare"]);
        assert!(area.contains("Ronfaure"), "{area}");
        let (l, area) = page(&d, r, 704).unwrap();
        assert_eq!((l, area.as_str()), (vec!["Siege Bat".to_string(), "Borer Beetle".to_string()], "Garlaige Citadel"));
    }

    /// Credits: by counts, then names; a family slot takes the monster no name fits; nothing fits a full slot.
    #[test]
    fn attribution() {
        let s = [slot("Siege Bat", 4, 2, true), slot("Borer Beetle", 2, 0, true)];
        assert_eq!(attribute(&s, 3, 4, None), Some(0));
        assert_eq!(attribute(&s, 1, 2, None), Some(1));
        let s = [slot("Goblin family", 4, 1, true), slot("Gigas family", 4, 1, true)];
        assert_eq!(attribute(&s, 2, 4, Some("Goblin Mugger")), Some(0));
        let s = [slot("Crab family", 6, 2, true), slot("Hill Lizard", 6, 2, true)];
        assert_eq!(attribute(&s, 3, 6, Some("Snipper")), Some(0));
        assert_eq!(attribute(&s, 3, 6, Some("Hill Lizard")), Some(1));
        // Counts unknown (a book's page, no kills yet): the name decides.
        let s = [slot("Siege Bat", 0, 0, false), slot("Borer Beetle", 0, 0, false)];
        assert_eq!(attribute(&s, 3, 4, Some("Borer Beetle")), Some(1));
        assert!(matches("Bat family", "Seeker Bats") && matches("Siege Bat", "siege bat") && !matches("Siege Bat", "Siege Bats"));
    }

    #[test]
    fn rows() {
        assert_eq!(row_text(&slot("Siege Bat", 4, 3, true)), ("Siege Bat: 3/4".into(), false, Some(0.75)));
        assert_eq!(row_text(&slot("Borer Beetle", 2, 2, true)).1, true);
        assert_eq!(row_text(&slot("Siege Bat", 0, 0, false)).0, "Siege Bat: ?/?");
    }
}
