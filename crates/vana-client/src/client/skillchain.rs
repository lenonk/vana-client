//! Ours (not retail; the skillchain add-ons): the skillchain helper. Each monster's resonance as 0x028 shows the party's
//! skills land, by LSB's GetSkillChainEffect: an opener leaves its properties for 10 s, a chain its own for 1 s less
//! per link, and a closer counts from 3 s after the last. Plus the skills each party member was seen using, and what
//! chains the party can make with all it has (dat/skillchains.rs for the properties and rules).

use super::World;
use super::game_ui::GameUi;
use super::timers::{Dock, Place};
use crate::dat::skillchains::{self as sc, Prop, Skill};
use crate::net::action::{Action, category};
use std::collections::HashMap;
use std::time::Instant;

/// A closer counts this long after the resonance started (LSB: `start + 3s < now`).
pub const WAIT: f32 = 3.0;
/// An opener's resonance lasts this long; each link takes a second off.
pub const LIFE: f32 = 10.0;
/// Skills remembered per member.
const SEEN: usize = 16;

#[derive(Clone, Debug)]
pub struct Resonance {
    /// An opener's properties, or the chain made (`chain`).
    pub props: [Prop; 3],
    pub chain: Option<Prop>,
    pub start: Instant,
    pub life: f32,
    pub links: u8,
    /// Who used what last on it.
    pub by: u32,
    pub skill: Skill,
}

impl Resonance {
    pub fn age(&self) -> f32 {
        self.start.elapsed().as_secs_f32()
    }

    pub fn left(&self) -> f32 {
        self.life - self.age()
    }

    /// The properties a closer pairs against.
    pub fn against(&self) -> Vec<Prop> {
        match self.chain {
            Some(c) => vec![c],
            None => self.props.iter().copied().filter(|&p| p != 0).collect(),
        }
    }
}

#[derive(Default)]
pub struct Tracker {
    pub on: HashMap<u32, Resonance>,
    /// Skills seen used, by actor (players, trusts, pets), newest last.
    pub seen: HashMap<u32, Vec<Skill>>,
    /// The window folded (the modern tab, the classic drawer in).
    pub folded: bool,
    /// Who each monster last went for in the party (its melee, spells and skills), and when.
    pub hits: HashMap<u32, (u32, Instant)>,
    /// The classic window: the drawer's slide 0 (in) .. 1 (out), this frame's lines and place, last frame's rects.
    slide: f32,
    last: Option<Instant>,
    frame: Option<(Vec<Line>, Place)>,
    rect: Option<[f32; 4]>,
    drawer: Option<[f32; 4]>,
}

/// A monster's last party target, while recent.
pub fn hitting(w: &World, mob: u32) -> Option<u32> {
    w.skillchain.hits.get(&mob).filter(|(_, at)| at.elapsed().as_secs_f32() < 8.0).map(|(t, _)| *t)
}

/// LSB's ActionProcSkillChain (the proc kind of a skill's record) as the chain's property.
fn proc_chain(kind: u8) -> Option<Prop> {
    Some(match kind {
        1 => 13,
        2 => 14,
        3 => 9,
        4 => 12,
        5 => 10,
        6 => 11,
        7 => 2,
        8 => 3,
        9 => 7,
        10 => 5,
        11 => 1,
        12 => 4,
        13 => 6,
        14 => 8,
        15 => 15,
        16 => 16,
        _ => return None,
    })
}

/// The skill an action finished, if it can carry properties.
fn skill_of(a: &Action) -> Option<Skill> {
    let id = a.arg as u16;
    match a.category {
        category::SKILL_FINISH => Some(Skill::Weapon(id)),
        category::MOB_SKILL_FINISH | 13 => Some(Skill::Mob(id)),
        category::MAGIC_FINISH if sc::data().blue.contains_key(&id) => Some(Skill::Blue(id)),
        _ => None,
    }
}

/// Us, a party member (trusts included) or a pet of either.
pub fn ours(w: &World, actor: u32) -> bool {
    super::dps::ours(w, actor)
}

/// 0x028: a party skill that hit a monster opens, continues or replaces its resonance.
pub fn on_action(w: &mut World, a: &Action) {
    if matches!(a.category, category::BASIC_ATTACK | category::MAGIC_FINISH | category::MOB_SKILL_FINISH)
        && w.entities.get(&a.actor).is_some_and(|e| e.mob)
        && let Some(&(t, _)) = a.records().first()
        && ours(w, t)
    {
        w.skillchain.hits.insert(a.actor, (t, Instant::now()));
    }
    let Some(skill) = skill_of(a) else { return };
    if !ours(w, a.actor) {
        return;
    }
    let props = sc::data().props(skill);
    let seen = w.skillchain.seen.entry(a.actor).or_default();
    seen.retain(|&s| s != skill);
    seen.push(skill);
    if seen.len() > SEEN {
        seen.remove(0);
    }
    for (target, r) in a.records() {
        if r.miss != 0 || ours(w, target) || !w.entities.get(&target).is_some_and(|e| e.mob) {
            continue;
        }
        let chain = r.proc.as_ref().and_then(|p| proc_chain(p.kind));
        let old = w.skillchain.on.get(&target);
        let res = match chain {
            Some(c) => {
                let life = old.filter(|o| o.left() > 0.0).map_or(LIFE, |o| o.life) - 1.0;
                Resonance { props: [c, 0, 0], chain: Some(c), start: Instant::now(), life, links: old.map_or(0, |o| o.links) + 1, by: a.actor, skill }
            }
            None if props.iter().any(|&p| p != 0) => Resonance { props, chain: None, start: Instant::now(), life: LIFE, links: 0, by: a.actor, skill },
            None => continue,
        };
        log::debug!("skillchain: {target:#x} {:?} by {:#x} {skill:?}", res.chain.map_or(res.props.to_vec(), |c| vec![c]), a.actor);
        w.skillchain.on.insert(target, res);
    }
}

/// Every frame: resonances that ran out or whose monster went.
pub fn tick(w: &mut World) {
    let ents = &w.entities;
    w.skillchain.on.retain(|id, r| r.left() > -1.0 && ents.get(id).is_some_and(|e| e.hpp > 0));
    w.skillchain.hits.retain(|id, (_, at)| at.elapsed().as_secs_f32() < 30.0 && ents.contains_key(id));
}

/// A party member and the skills they have: ours from our list, a trust's from its list, anyone's as seen.
#[derive(Clone, Debug)]
pub struct Member {
    pub name: String,
    pub me: bool,
    pub tp: u32,
    pub skills: Vec<Skill>,
}

/// The party (us first) with their skills; members with none are left out.
pub fn members(w: &World) -> Vec<Member> {
    let d = sc::data();
    let mut out = Vec::new();
    let mine: Vec<Skill> = super::magic::ability_rows(w, 3)
        .into_iter()
        .filter_map(|i| w.actions.as_ref()?.abilities.get(i as usize).map(|a| Skill::Weapon(a.id)))
        .filter(|s| d.props(*s).iter().any(|&p| p != 0))
        .collect();
    out.push(Member { name: w.name.clone(), me: true, tp: w.vitals.map_or(0, |v| v.2), skills: mine });
    for m in w.party.party().filter(|m| m.id != w.my_id) {
        let trust = w.entities.get(&m.id).is_some_and(|e| e.marks.trust);
        let mut skills: Vec<Skill> = if trust { d.trust(&m.name).map_or(Vec::new(), |ids| ids.iter().map(|&i| Skill::Mob(i)).collect()) } else { Vec::new() };
        for &s in w.skillchain.seen.get(&m.id).into_iter().flatten() {
            if !skills.contains(&s) {
                skills.push(s);
            }
        }
        skills.retain(|s| d.props(*s).iter().any(|&p| p != 0));
        // A trust's list can hold one skill under several ids (its level variants).
        let mut names: Vec<String> = Vec::new();
        skills.retain(|&s| {
            let n = name(w, s);
            let new = !names.contains(&n);
            names.push(n);
            new
        });
        out.push(Member { name: m.name.clone(), me: false, tp: m.tp, skills });
    }
    out
}

/// A skill's name: the client's for weapon skills (and mob skills in that range), else LSB's.
pub fn name(w: &World, s: Skill) -> String {
    let id = match s {
        Skill::Weapon(id) => Some(id),
        Skill::Mob(id) if id < 256 => Some(id),
        _ => None,
    };
    if let Some(id) = id {
        let n = super::magic::ability_name(w, id);
        if !n.is_empty() {
            return n;
        }
    }
    if let Skill::Blue(id) = s {
        return super::magic::spell_name(w, id);
    }
    sc::data().get(s).map_or_else(String::new, |e| sc::pretty(&e.name))
}

/// One way to chain: who opens with what, who closes with what, and the chain.
#[derive(Clone, Debug, PartialEq)]
pub struct Combo {
    pub opener: (usize, Skill),
    pub closer: (usize, Skill),
    pub chain: Prop,
}

/// Every chain two different members can make (by `members` index), the strongest first, then those with us in
/// them; per pair of members and chain only the first found.
pub fn combos(members: &[Member]) -> Vec<Combo> {
    let d = sc::data();
    let mut out: Vec<Combo> = Vec::new();
    for (a, ma) in members.iter().enumerate() {
        for (b, mb) in members.iter().enumerate().filter(|(b, _)| *b != a) {
            for &sa in &ma.skills {
                for &sb in &mb.skills {
                    let Some(chain) = sc::form(&d.props(sa), &d.props(sb)) else { continue };
                    if !out.iter().any(|c| c.opener.0 == a && c.closer.0 == b && c.chain == chain) {
                        out.push(Combo { opener: (a, sa), closer: (b, sb), chain });
                    }
                }
            }
        }
    }
    out.sort_by_key(|c| (std::cmp::Reverse(sc::level(c.chain)), c.opener.0.min(c.closer.0), c.opener.0, c.closer.0));
    out
}

/// What closes a resonance: (member index, skill, chain), the strongest first.
pub fn closers(members: &[Member], res: &Resonance) -> Vec<(usize, Skill, Prop)> {
    let d = sc::data();
    let against = res.against();
    let against = &against;
    let mut out: Vec<(usize, Skill, Prop)> = members
        .iter()
        .enumerate()
        .flat_map(|(k, m)| m.skills.iter().filter_map(move |&s| sc::form(against, &d.props(s)).map(|c| (k, s, c))))
        .collect();
    out.sort_by_key(|&(k, s, c)| (std::cmp::Reverse(sc::level(c)), k, s));
    out
}

/// The monster the helper follows: our target when it is a monster, else the one resonating most recently.
pub fn focus(w: &World) -> Option<u32> {
    w.target.filter(|t| w.entities.get(t).is_some_and(|e| e.mob)).or_else(|| w.skillchain.on.iter().max_by_key(|(_, r)| r.start).map(|(id, _)| *id))
}

/// What the helper shows, ready to draw: the open resonance on the focus monster, then every chain the party can make.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub now: Option<Now>,
    pub chains: Vec<ChainWays>,
}

/// The resonance on the focus monster: its properties or the chain made, who did it, its timing and what closes it.
#[derive(Clone, Debug, Default)]
pub struct Now {
    pub mob: String,
    pub props: Vec<Prop>,
    pub chain: Option<Prop>,
    pub by: String,
    pub age: f32,
    pub left: f32,
    pub life: f32,
    pub closers: Vec<Closer>,
}

#[derive(Clone, Debug)]
pub struct Closer {
    pub who: String,
    pub skill: String,
    pub chain: Prop,
    pub ready: bool,
}

/// One chain and every way the party makes it: ours first, then those whose opener has the TP.
#[derive(Clone, Debug)]
pub struct ChainWays {
    pub chain: Prop,
    pub ways: Vec<Way>,
}

#[derive(Clone, Debug)]
pub struct Way {
    pub opener: (String, String),
    pub closer: (String, String),
    pub ready: bool,
}

/// The helper's view, in a party of two or more with skills known.
pub fn view(w: &World) -> Option<View> {
    let members = members(w);
    if members.len() < 2 {
        return None;
    }
    let who = |k: usize| if members[k].me { "You".to_string() } else { members[k].name.clone() };
    let ready = |k: usize| members[k].tp >= 1000;
    let focus = focus(w);
    let now = focus.and_then(|t| w.skillchain.on.get(&t)).filter(|r| r.left() > 0.0).map(|r| {
        let mob = focus.and_then(|t| w.entities.get(&t)).map(|e| super::monstrosity::name(e).to_string()).unwrap_or_default();
        let by = members.iter().position(|x| x.me && r.by == w.my_id).map(|k| who(k)).or_else(|| w.party.party().find(|x| x.id == r.by).map(|x| x.name.clone())).unwrap_or_default();
        let closers = closers(&members, r).into_iter().map(|(k, s, c)| Closer { who: who(k), skill: name(w, s), chain: c, ready: ready(k) }).collect();
        Now { mob, props: r.props.iter().copied().filter(|&p| p != 0).collect(), chain: r.chain, by: if by.is_empty() { name(w, r.skill) } else { format!("{by}: {}", name(w, r.skill)) }, age: r.age(), left: r.left(), life: r.life, closers }
    });
    let combos = combos(&members);
    let mut chains: Vec<ChainWays> = Vec::new();
    for cb in &combos {
        if chains.iter().any(|c| c.chain == cb.chain) {
            continue;
        }
        let mut ways: Vec<&Combo> = combos.iter().filter(|x| x.chain == cb.chain).collect();
        ways.sort_by_key(|x| (!(members[x.opener.0].me || members[x.closer.0].me), !ready(x.opener.0)));
        let ways = ways.into_iter().map(|x| Way { opener: (who(x.opener.0), name(w, x.opener.1)), closer: (who(x.closer.0), name(w, x.closer.1)), ready: ready(x.opener.0) }).collect();
        chains.push(ChainWays { chain: cb.chain, ways });
    }
    Some(View { now, chains })
}

/// "Fusion (Light, Fire)".
pub fn chain_text(p: Prop) -> String {
    let els: Vec<&str> = sc::elements(p).iter().map(|e| e.name()).collect();
    format!("{} ({})", sc::NAMES[p as usize], els.join(", "))
}

/// A skill's properties as text: "Transfixion / Scission".
pub fn props_text(props: &[Prop]) -> String {
    props.iter().filter(|&&p| p != 0).map(|&p| sc::NAMES[p as usize]).collect::<Vec<_>>().join(" / ")
}

// ---- The window (classic) ----

/// The window's width (wider than the Timers': skill names) and pitches.
const WIDTH: f32 = 240.0;
const PAD: f32 = 8.0;
const HEAD_H: f32 = 16.0;
const LINE_H: f32 = 15.0;
const SLIVER: f32 = 22.0;
const SLIDE_TICKS: f32 = 12.0;
const TOP_EDGE: f32 = 48.0;
const N: u32 = 0x8080_8080;
const GOLD: u32 = 0x80a0_a080;
const MUTED: u32 = 0x8058_5858;

/// One line of the classic window.
#[derive(Clone, Debug)]
pub enum Line {
    Head(String),
    Text(String, u32),
    /// A gauge: filled part and its colour.
    Bar(f32, [u8; 4]),
}

fn line_h(l: &Line) -> f32 {
    match l {
        Line::Head(_) => HEAD_H,
        Line::Text(..) => LINE_H,
        Line::Bar(..) => 10.0,
    }
}

fn level_tint(c: Prop) -> u32 {
    match sc::level(c) {
        3 | 4 => N,
        2 => GOLD,
        _ => 0x8070_7070,
    }
}

/// The classic window's lines: the open resonance on the focus monster and its closers, then the party's chains.
pub fn lines(w: &World) -> Vec<Line> {
    let members = members(w);
    let mut out = Vec::new();
    if members.len() < 2 {
        return out;
    }
    let who = |k: usize| if members[k].me { "You".to_string() } else { members[k].name.clone() };
    let ready = |k: usize| members[k].tp >= 1000;
    let focus = focus(w);
    if let Some(r) = focus.and_then(|t| w.skillchain.on.get(&t)).filter(|r| r.left() > 0.0) {
        let mob = focus.and_then(|t| w.entities.get(&t)).map(|e| super::monstrosity::name(e).to_string()).unwrap_or_default();
        match r.chain {
            Some(c) => {
                out.push(Line::Text(format!("Skillchain: {}", sc::NAMES[c as usize]), GOLD));
                let els: Vec<&str> = sc::elements(c).iter().map(|e| e.name()).collect();
                out.push(Line::Text(format!("Burst {}: {:.1} s", els.join(" / "), r.left()), N));
            }
            None => {
                out.push(Line::Text(format!("{}: {}", mob, props_text(&r.props)), N));
            }
        }
        let age = r.age();
        if age < WAIT {
            out.push(Line::Text(format!("Wait {:.1} s", WAIT - age), MUTED));
            out.push(Line::Bar(age / WAIT, [0x40, 0x40, 0x40, 0xa9]));
        } else {
            out.push(Line::Text(format!("Close within {:.1} s", r.left()), MUTED));
            out.push(Line::Bar(r.left() / (r.life - WAIT).max(0.1), [0x80, 0x70, 0x20, 0xa9]));
        }
        for (k, s, c) in closers(&members, r).into_iter().take(4) {
            out.push(Line::Text(format!("{}: {}", who(k), name(w, s)), if ready(k) { N } else { MUTED }));
            out.push(Line::Text(format!("  > {}", chain_text(c)), level_tint(c)));
        }
    }
    let combos = combos(&members);
    out.push(Line::Head("Party chains".into()));
    if combos.is_empty() {
        out.push(Line::Text("None known yet.".into(), MUTED));
    }
    let mut chains: Vec<Prop> = Vec::new();
    for cb in &combos {
        if chains.contains(&cb.chain) {
            continue;
        }
        chains.push(cb.chain);
        let best = combos.iter().filter(|x| x.chain == cb.chain).min_by_key(|x| (!(members[x.opener.0].me || members[x.closer.0].me), !ready(x.opener.0))).unwrap_or(cb);
        out.push(Line::Text(chain_text(cb.chain), level_tint(cb.chain)));
        out.push(Line::Text(format!("{}: {}", who(best.opener.0), name(w, best.opener.1)), if ready(best.opener.0) { N } else { MUTED }));
        out.push(Line::Text(format!("  > {}: {}", who(best.closer.0), name(w, best.closer.1)), MUTED));
    }
    out
}

/// Each frame from the HUD after the training regime's prepare: hidden in events, under the map, and out of a party.
pub fn prepare(w: &mut World, base: Place) {
    let now = Instant::now();
    let dt = w.skillchain.last.map_or(0.0, |l| now.duration_since(l).as_secs_f32());
    w.skillchain.last = Some(now);
    let step = dt * 60.0 / SLIDE_TICKS;
    let out = !w.skillchain.folded;
    w.skillchain.slide = if out { (w.skillchain.slide + step).min(1.0) } else { (w.skillchain.slide - step).max(0.0) };
    w.skillchain.frame = None;
    if !w.extra_skillchain || super::event::active(w) || w.map.is_some() || w.cnq.is_some() {
        return;
    }
    let lines = lines(w);
    if lines.is_empty() {
        return;
    }
    // Under the Timers and the training regime windows (above them on the right).
    let over = [if w.extra_timers { w.timers.rect() } else { None }, if w.extra_regime { w.regime.rect() } else { None }];
    let place = over.iter().flatten().fold(base, |p, r| match p.dock {
        Dock::Right if r[1] - 4.0 < p.y => Place { y: r[1] - 4.0, max_h: (p.max_h - (p.y - r[1] + 4.0)).max(0.0), ..p },
        Dock::Right => p,
        _ if r[1] + r[3] + 4.0 > p.y => Place { y: r[1] + r[3] + 4.0, max_h: (p.max_h - (r[1] + r[3] + 4.0 - p.y)).max(0.0), ..p },
        _ => p,
    });
    w.skillchain.frame = Some((lines, place));
}

fn ease(s: f32) -> f32 {
    s * s * (3.0 - 2.0 * s)
}

/// Draws the window at `p` with the drawer at `slide`; its rect and the drawer's.
pub fn draw_panel(gu: &mut GameUi, lines: &[Line], p: &Place, slide: f32, sw: f32) -> Option<([f32; 4], [f32; 4])> {
    let mut shown = 0;
    let mut h = 2.0 * PAD + HEAD_H;
    for l in lines {
        if h + line_h(l) > p.max_h {
            break;
        }
        h += line_h(l);
        shown += 1;
    }
    if p.max_h < 2.0 * PAD + HEAD_H {
        return None;
    }
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
    let (rx, inner) = (x + PAD, WIDTH - 2.0 * PAD);
    let hx = if p.dock == Dock::Right { rx + 16.0 } else { rx };
    gu.shape_fit("Skillchains", hx, y + PAD + 1.0, inner - 22.0, GOLD);
    let mut ly = y + PAD + HEAD_H;
    for l in lines.iter().take(shown) {
        match l {
            Line::Head(t) => {
                gu.shape_fit(t, rx, ly + 1.0, inner, GOLD);
            }
            Line::Text(t, tint) => gu.text_fit(t, rx, ly - 3.0, inner, *tint),
            Line::Bar(f, rgba) => {
                if *f > 0.0 {
                    gu.sprite_coloured("menu    windowps", 97, rx + 4.0, ly + 1.0, f.min(1.0), 1.0, *rgba, 8);
                }
                gu.sprite("menu    windowps", 38, rx, ly, N);
            }
        }
        ly += line_h(l);
    }
    gu.clip(None);
    Some((rect, drawer))
}

pub fn draw(gu: &mut GameUi, t: &mut Tracker, sw: f32) {
    (t.rect, t.drawer) = (None, None);
    let Some((lines, p)) = t.frame.take() else { return };
    if let Some((rect, drawer)) = draw_panel(gu, &lines, &p, t.slide, sw) {
        (t.rect, t.drawer) = (Some(rect), Some(drawer));
    }
}

/// After every window: a click on the drawer's arrow, unless a window over ours has the spot. The window's rect for
/// the world-click blockers.
pub fn pointer(w: &mut World, gu: &GameUi, p: Option<(f32, f32)>, clicked: bool, over: &[[f32; 4]]) -> Option<[f32; 4]> {
    let r = w.skillchain.rect?;
    let Some(p) = p else { return Some(r) };
    let inside = |r: &[f32; 4]| p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3];
    if !inside(&r) || over.iter().any(inside) || !w.skillchain.drawer.as_ref().is_some_and(inside) {
        return Some(r);
    }
    gu.hover.set(true);
    if clicked {
        w.skillchain.folded = !w.skillchain.folded;
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    Some(r)
}

/// topbar-shot's sample (`--window skillchain:DOCK[:SLIDE]`): a Liquefaction made on a monster, two trusts.
pub fn shot(gu: &mut GameUi, spec: &str, sw: f32, sh: f32, mini: Option<[f32; 4]>) {
    let f: Vec<&str> = spec.split(':').collect();
    let dock = Dock::of(f.first().and_then(|d| d.parse().ok()).unwrap_or(0));
    let slide = f.get(1).and_then(|d| d.parse().ok()).unwrap_or(1.0);
    let lines = vec![
        Line::Text("Skillchain: Liquefaction".into(), GOLD),
        Line::Text("Burst Fire: 5.5 s".into(), N),
        Line::Text("Close within 5.5 s".into(), MUTED),
        Line::Bar(0.7, [0x80, 0x70, 0x20, 0xa9]),
        Line::Text("Naji: Vorpal Blade".into(), N),
        Line::Text(format!("  > {}", chain_text(11)), level_tint(11)),
        Line::Text("You: Fast Blade".into(), MUTED),
        Line::Text(format!("  > {}", chain_text(4)), level_tint(4)),
        Line::Head("Party chains".into()),
        Line::Text(chain_text(11), level_tint(11)),
        Line::Text("You: Burning Blade".into(), N),
        Line::Text("  > Naji: Vorpal Blade".into(), MUTED),
        Line::Text(chain_text(9), level_tint(9)),
        Line::Text("You: Red Lotus Blade".into(), N),
        Line::Text("  > Ayame: Tachi: Kasha".into(), MUTED),
    ];
    let log_top = sh - 16.0 - 8.0 * 16.0 - 14.0;
    let foot = sh - 16.0 - 34.0 - 46.0;
    let p = super::timers::place(dock, sw, sh, 4, log_top, foot, mini);
    draw_panel(gu, &lines, &p, slide, sw);
}
