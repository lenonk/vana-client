//! `modern-shot`: the modern layer offscreen without a login: the HUD panels with sample values and every window frame
//! at several sizes, over a screenshot when one is given.

use super::chat::{self, ChatView, InputView};
use super::super::combat::NO_MODE;
use super::hud::{self, Effect, Icon, PartyRow, StatusView, TargetView, WhereView};
use super::{Layout, Modern, Panels, Title, kit, paint};
use crate::render::Gpu;
use anyhow::{Context as _, Result};
use egui::{Color32, Id, LayerId, Order, Pos2, Rect, pos2, vec2};
use std::path::Path;

fn load_png(path: &Path) -> Result<([usize; 2], Vec<u8>)> {
    let mut d = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).with_context(|| path.display().to_string())?));
    d.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut r = d.read_info()?;
    let mut buf = vec![0u8; r.output_buffer_size().unwrap_or(0)];
    let info = r.next_frame(&mut buf)?;
    buf.truncate(info.buffer_size());
    anyhow::ensure!(info.color_type == png::ColorType::Rgba, "{}: not RGBA after expansion", path.display());
    Ok(([info.width as usize, info.height as usize], buf))
}

/// A list as the menus draw theirs: rows in the pack's art, the name at the left, the count at the right.
fn list_sample(p: &mut Panels) {
    let m = p.m;
    let rows = [("Hi-Potion", 12), ("Echo Drops", 4), ("Fire Crystal", 99), ("Scroll of Protect II", 1), ("Sleep Bolt", 87), ("Mithkabob", 6)];
    let at = p.ctx.content_rect().center() + vec2(-160.0, -240.0);
    p.panel_at("items", Some(super::Title { icon: Some("icons/items"), text: "Items", note: Some("42/80") }), 320.0, at, |ui| {
        for (k, (name, n)) in rows.iter().enumerate() {
            let (r, _) = ui.allocate_exact_size(vec2(296.0, 28.0), egui::Sense::hover());
            let face = if k == 2 { "controls/list_row_selected" } else { "controls/list_row_normal" };
            super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
            ui.painter().text(pos2(r.min.x + 24.0, r.center().y), egui::Align2::LEFT_CENTER, *name, egui::FontId::proportional(m.theme.small), m.theme.color("text"));
            ui.painter().text(pos2(r.max.x - 12.0, r.center().y), egui::Align2::RIGHT_CENTER, n.to_string(), egui::FontId::proportional(m.theme.small), m.theme.color("muted"));
            ui.add_space(2.0);
        }
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Restores 100 HP.").size(m.theme.small).color(m.theme.color("muted")));
    });
}

/// The favourites strip (no game art offscreen: the names' first letters) and list rows with their stars.
fn favs_sample(p: &mut Panels) {
    use super::super::favs::Fav;
    let chip = |fav, name: &str, off, count| super::favs::Chip { fav, name: name.into(), icon: None, off, count };
    let chips = [chip(Fav::Spell(1), "Cure", false, None), chip(Fav::Spell(4), "Cure IV", true, None), chip(Fav::Ability(0x210), "Provoke", false, None), chip(Fav::Ability(0x2a), "Savage Blade", false, None), chip(Fav::Item(4112), "Hi-Potion", false, Some(12)), chip(Fav::Item(4151), "Echo Drops", false, Some(4))];
    super::favs::strip(p, &chips);
    let m = p.m;
    let at = p.ctx.content_rect().center() + vec2(-160.0, -240.0);
    p.panel_at("items", Some(super::Title { icon: None, text: "White Magic", note: Some("4") }), 320.0, at, |ui| {
        for (k, (name, on)) in [("Cure", true), ("Cure II", false), ("Cure IV", true), ("Protect", false)].into_iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(296.0, 30.0), egui::Sense::hover());
            let face = if k == 1 { "controls/list_row_selected" } else { "controls/list_row_normal" };
            super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
            ui.painter().text(pos2(r.min.x + 24.0, r.center().y), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(m.theme.ui), m.theme.color("text"));
            super::favs::star(ui, m, r, on, k == 1 || resp.hovered());
            ui.add_space(2.0);
        }
    });
}

/// The main menu as command.rs draws it, Journal open with its submenu: a hovered row, one disabled.
/// The kit's controls as a Config page has them: sliders with a short and a long value, options, a long-labelled line.
fn kit_sample(p: &mut Panels) {
    let m = p.m;
    p.panel("window", Some(Title { icon: Some("icons/config"), text: "Config", note: None }), 640.0, |ui| {
        ui.set_width(440.0);
        kit::slider_setting(ui, m, "Resolution", false, false, 17, 16, "2560x1440");
        kit::slider_setting(ui, m, "GPU", true, false, 3, 1, "AMD Radeon RX 6800 XT (RADV NAVI21)");
        kit::setting(ui, m, "3D Aspect", false, true, false, |ui| kit::choice(ui, m, &["Window".to_string(), "Config".to_string()], 1, 0));
        kit::line(ui, m, "1. a-pack-with-a-rather-long-folder-name-from-an-archive-2019-final", false, false, |ui| {
            kit::button(ui, m, "Uninstall", 0.0, kit::Btn::default());
            kit::button(ui, m, "ON", 64.0, kit::Btn { lit: true, ..Default::default() });
        });
    });
    // link11 as the modern window draws it (no item cache here: a disc stands in for the shell icon).
    let v = super::social::NameView { name: "Sunset Watch", rgb: [15, 9, 3], sel: 3, edit: 0 };
    p.panel_at("naming sample", Some(Title { icon: Some("icons/linkshell"), text: "New Linkshell", note: None }), super::social::NAME_W + 44.0, pos2(720.0, 40.0), |ui| {
        super::social::naming_body(ui, m, &v, None, true);
    });
}

fn menu_sample(p: &mut Panels) {
    let m = p.m;
    let [top, journal] = super::command::sample();
    let at = p.ctx.content_rect().right_top() + vec2(-290.0, 60.0);
    let mut rect = None;
    p.panel_at("command", Some(super::Title { icon: None, text: "Menu", note: None }), 250.0, at, |ui| {
        let r = ui.min_rect();
        for (label, icon, sub) in &top {
            super::rows::menu_row(ui, m, icon, label, *label == "Journal", false, *sub, true, *label == "Magic");
        }
        rect = Some(r);
    });
    let Some(r) = rect else { return };
    p.panel_at("command sub", Some(super::Title { icon: Some("icons/quest_log"), text: "Journal", note: None }), 250.0, pos2(r.min.x - 274.0, r.min.y - 12.0), |ui| {
        for (label, icon, sub) in &journal {
            super::rows::menu_row(ui, m, icon, label, *label == "Quests", *label == "Key Items", *sub, false, false);
        }
    });
}

/// An edge window on the right edge under the menu (Wide Scan's look, a few rows), in its frame.
fn edge_sample(m: &Modern, ctx: &egui::Context) {
    let screen = ctx.content_rect();
    let r = Rect::from_min_max(pos2(screen.max.x - 290.0, 560.0), pos2(screen.max.x, 800.0));
    let p = ctx.layer_painter(LayerId::new(Order::Middle, Id::unique("edge sample")));
    p.add(super::extras::edge_frame_shape(r, super::extras::Side::Right, true, m, true, 16));
    let font = egui::FontId::new(m.theme.small + 1.0, egui::FontFamily::Name(super::STRONG.into()));
    p.text(pos2(r.min.x + 36.0, r.min.y + 22.0), egui::Align2::LEFT_CENTER, "Wide Scan (37)", font, m.theme.color("gold"));
    for (k, (name, d)) in [("Rumoie", 0), ("Fulchia", 15), ("Home Point #3", 29), ("Trail Markings", 31), ("Artisan Moogle", 45), ("Lotte", 62), ("Valderotaux", 64)].iter().enumerate() {
        let y = r.min.y + 52.0 + 22.0 * k as f32;
        p.text(pos2(r.min.x + 22.0, y), egui::Align2::LEFT_CENTER, *name, egui::FontId::proportional(m.theme.small), Color32::from_rgb(0x7c, 0xe0, 0x8a));
        p.text(pos2(r.max.x - 30.0, y), egui::Align2::RIGHT_CENTER, d.to_string(), egui::FontId::proportional(m.theme.small), m.theme.color("muted"));
    }
}

/// The skillchain helper on the left edge: a Liquefaction just made, three trusts and long names.
fn skillchain_sample(m: &Modern, ctx: &egui::Context) {
    use crate::client::skillchain::{ChainWays, Closer, Now, View, Way};
    let way = |a: &str, sa: &str, b: &str, sb: &str, ready: bool| Way { opener: (a.into(), sa.into()), closer: (b.into(), sb.into()), ready, chain: 0 };
    let closer = |who: &str, skill: &str, chain, ready| Closer { who: who.into(), skill: skill.into(), chain, ready };
    let v = View {
        now: Some(Now {
            mob: "Bloodthirsty Troll".into(),
            props: vec![3],
            chain: Some(3),
            by: "Shantotto II: Final Exquisite Exquisite".into(),
            age: 4.2,
            left: 4.8,
            life: 9.0,
            closers: vec![closer("You", "Blade: Retsu", 11, true), closer("Ayame", "Tachi: Kasha", 11, false), closer("Naji", "Vorpal Blade", 4, true), closer("Apururu (UC)", "Nott", 8, false)],
        }),
        chains: vec![
            ChainWays { chain: 14, ways: vec![Way { chain: 16, ..way("You", "Blade: Chi", "Ayame", "Tachi: Yukikaze", true) }, Way { chain: 14, ..way("Naji", "Vorpal Blade", "You", "Blade: Ten", false) }, Way { chain: 14, ..way("Ayame", "Tachi: Gekko", "Naji", "Burning Blade", false) }] },
            ChainWays { chain: 11, ways: vec![way("You", "Blade: Retsu", "Shantotto II", "Final Exquisite Exquisite", true), way("Naji", "Red Lotus Blade", "Ayame", "Tachi: Kasha", false)] },
            ChainWays { chain: 12, ways: vec![way("Ayame", "Tachi: Enpi", "You", "Blade: Jin", false)] },
            ChainWays { chain: 3, ways: vec![way("Naji", "Fast Blade", "You", "Blade: Teki", true), way("You", "Blade: Rin", "Naji", "Burning Blade", true), way("Shantotto II", "Shantotto Pson", "You", "Blade: To", false), way("Ayame", "Tachi: Hobaku", "Naji", "Red Lotus Blade", false)] },
        ],
    };
    m.sc_open.borrow_mut().extend([3, 14]);
    let width = super::extras::skillchain_width(ctx, m, &v);
    let screen = ctx.content_rect();
    let at = pos2(screen.min.x, 120.0);
    let shown = egui::Area::new(Id::unique("skillchain sample")).order(Order::Middle).fixed_pos(at).constrain(false).show(ctx, |ui| {
        let bg = ui.painter().add(egui::Shape::Noop);
        let inner = egui::Frame::NONE.inner_margin(egui::Margin { left: 12, right: 18, top: 12, bottom: 14 }).show(ui, |ui| {
            ui.set_width(width - 30.0);
            ui.visuals_mut().override_text_color = Some(m.theme.color("text"));
            ui.label(egui::RichText::new("Skillchains").size(m.theme.small + 1.0).color(m.theme.color("gold")));
            super::extras::skillchain_body(ui, m, &v);
        });
        ui.painter().set(bg, super::extras::edge_frame_shape(inner.response.rect, super::extras::Side::Left, true, m, true, 16));
    });
    let _ = shown;
}

/// The Timers at `x` (`width` wide, held by `side`): recasts, a full party's buffs and two monsters' debuffs, pack
/// icons standing in for the status icons.
fn timers_sample(m: &Modern, ctx: &egui::Context, x: f32, width: f32, side: super::extras::Side) {
    use super::super::timers::{Group, Icon, Row};
    let row = |name: &str, left: f32, total: f32, fills: bool| Row { icon: Icon::None, name: name.into(), left, total, fills };
    let fx = |name: &str, left: f32, total: f32| row(name, left, total, false);
    let party = |name: &str, rows: Vec<Row>| Group { name: Some(name.into()), rows };
    let songs = |a: f32| vec![fx("Victory March", a, 180.0), fx("Valor Minuet IV", a + 2.0, 180.0), fx("Chaos Roll (4)", 252.0, 300.0), fx("Samurai Roll (9)", 270.0, 300.0)];
    let with = |mut v: Vec<Row>, more: Vec<Row>| {
        v.extend(more);
        v
    };
    let secs: [Vec<Group>; 3] = [
        vec![Group { name: None, rows: vec![row("Provoke", 12.0, 30.0, true), row("Cure IV", 4.0, 6.0, true), row("Phantom Roll (1)", 31.0, 60.0, true)] }],
        vec![
            party("Lenon", with(vec![fx("Protect V", 1452.0, 1800.0), fx("Shell V", 1452.0, 1800.0), fx("Haste", 171.0, 180.0), fx("Refresh", 108.0, 150.0)], songs(100.0))),
            party("Nonopiko", with(vec![fx("Haste", 65.0, 180.0), fx("Refresh", 130.0, 150.0), fx("Protect V", 1500.0, 1800.0), fx("Shell V", 1500.0, 1800.0)], songs(100.0))),
            party("Kazuto", with(vec![fx("Haste", 12.0, 180.0)], songs(98.0))),
            party("Shantotto II", vec![fx("Mage's Ballad II", 122.0, 180.0), fx("Mage's Ballad", 124.0, 180.0), fx("Chaos Roll (4)", 252.0, 300.0), fx("Refresh", 0.0, 150.0)]),
            party("Apururu (UC)", vec![fx("Mage's Ballad II", 122.0, 180.0), fx("Mage's Ballad", 124.0, 180.0)]),
            party("Raven", with(vec![fx("Reraise", 3540.0, 3600.0)], songs(97.0))),
        ],
        vec![
            party("Bloodthirsty Troll", vec![fx("Dia III", 105.0, 180.0), fx("Slow II", 160.0, 180.0), fx("Paralyze II", 125.0, 180.0), fx("Blind II", 140.0, 180.0), fx("Silence", 25.0, 120.0), fx("Addle", 98.0, 180.0), fx("Distract II", 150.0, 180.0), fx("Frazzle", 15.0, 180.0), fx("Carnage Elegy", 0.0, 0.0)]),
            party("Forest Hare", vec![fx("Dia", 45.0, 60.0), fx("Poison", 8.0, 30.0)]),
        ],
    ];
    let pack = |name: &str| match name.split_whitespace().next().unwrap_or("") {
        "Protect" => "icons/protect",
        "Shell" => "icons/shield",
        "Haste" => "icons/haste",
        "Refresh" => "icons/refresh",
        "Victory" | "Valor" | "Mage's" | "Carnage" => "icons/songs",
        "Chaos" | "Samurai" => "icons/skillchain",
        "Reraise" => "icons/raise",
        "Silence" => "icons/silence",
        "Poison" => "icons/poison",
        "Provoke" => "icons/enmity",
        "Cure" => "icons/cure",
        "Phantom" => "icons/recast",
        _ => "icons/enfeebling",
    };
    let icon = |p: &egui::Painter, (k, gi, ri): (usize, usize, usize), r: Rect, tint: Color32| {
        if let Some(s) = m.art.sprite(pack(&secs[k][gi].rows[ri].name)) {
            p.add(paint::image(&s, r.min, r.width(), tint));
        }
    };
    let margin = if side == super::extras::Side::Right { egui::Margin { left: 18, right: 12, top: 12, bottom: 14 } } else { egui::Margin { left: 16, right: 16, top: 12, bottom: 14 } };
    egui::Area::new(Id::unique(("timers sample", x as i32))).order(Order::Middle).fixed_pos(pos2(x, 40.0)).constrain(false).show(ctx, |ui| {
        let bg = ui.painter().add(egui::Shape::Noop);
        let inner = egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
            ui.set_width(width - margin.sum().x);
            ui.visuals_mut().override_text_color = Some(m.theme.color("text"));
            ui.label(egui::RichText::new("Timers").size(m.theme.small + 1.0).color(m.theme.color("gold")));
            super::extras::timers_body(ui, m, &secs, 0, 0.5, &icon);
        });
        let attached = side == super::extras::Side::Right;
        ui.painter().set(bg, super::extras::edge_frame_shape(inner.response.rect, side, attached, m, attached, 16));
    });
}

/// Title rows whose notes are long: the Commands window on a target whose name takes two lines, on one with a ':',
/// on a short one, and a target with a long name and a distance.
fn titles_sample(p: &mut Panels) {
    let m = p.m;
    let rows = |ui: &mut egui::Ui, labels: &[&str]| {
        for l in labels {
            ui.label(egui::RichText::new(*l).size(m.theme.ui).color(m.theme.color("text")));
        }
    };
    for (id, title, note) in [("combat", "Commands", "Ghillie Dhu of the Great Woods"), ("palette", "Battle", "Qu'Vho: Deathhurler"), ("list", "Commands", "Tonberry Creeper")] {
        p.panel(id, Some(Title { icon: Some("icons/attack"), text: title, note: Some(note) }), 270.0, |ui| rows(ui, &["Attack", "Magic", "Abilities"]));
    }
    hud::target_panel(p, &TargetView { name: "Shadow Lord's Lieutenant of the North".into(), ring: "indicators/target_hostile", locked: false, hp: Some((100, 1.0)), colour: None, cast: None, distance: Some(4.2) });
}

fn sample(m: &Modern, ctx: &egui::Context, layout: &mut Layout, opts: &Opts) {
    let top = crate::client::hudtop::TopView {
        title: "Status".into(),
        help: "View status.".into(),
        alpha: 1.0,
        shown: true,
        title_up: true,
        net_up: true,
        busy: None,
        pct: 100,
        sent: 154,
        recv: 276,
        meter: (1, 3),
        mail: None,
        gm_reply: false,
    };
    super::topbar::draw(m, ctx, &top);
    let (mut changed, mut chat_font) = (false, opts.chat_font);
    let mut p = Panels { m, ctx, gu: None, layout, vis: Default::default(), changed: &mut changed, chat_font: &mut chat_font, battle_in_all: &mut true, condense: &mut true, folded: &mut Default::default(), scan_kinds: &mut 7, this: Default::default(), retail: Default::default(), focus: Some("target"), hidden: Vec::new() };
    // The chat window first, as the layer draws it: the location panel sits on it.
    let wh = [0x80, 0x80, 0x80, 0x80];
    let lines = vec![
        (String::new(), "=== Area: West Ronfaure ===", wh, NO_MODE),
        ("[18:42:10] ".to_string(), "Raven : rdy?", [0x80, 0x80, 0x80, 0x80], 9u16),
        (String::new(), "Lenon starts casting Protect on Koru.", [0x30, 0x78, 0x80, 0x80], NO_MODE),
        (String::new(), "Koru gains the effect of Protect.", wh, NO_MODE),
        (String::new(), "The Robber Crab hits Raven for 24 points of damage.", [0x60, 0x60, 0x80, 0x80], NO_MODE),
        ("[18:42:31] ".to_string(), "(Koru) \u{E008}Let's go!\u{E009}", [0x80, 0x70, 0x40, 0x80], 13),
        ("[18:42:33] ".to_string(), "(Raven) lenon, cure me pls", [0x80, 0x70, 0x40, 0x80], 13),
        // Japanese game text (zone 70's Sakura) through the system CJK font.
        (String::new(), "Sakura : \u{30D4}\u{30E8}\u{30D4}\u{30E8}\u{FF01} \u{65E5}\u{672C}\u{8A9E}", wh, NO_MODE),
        // The game's own glyphs and an event item name (1e 02 .. 1e 01).
        (String::new(), "Kupipi : A pre\u{FF5E}sent? Take this \u{100402}Fire Crystal\u{100401} to the \u{F87B2}Mog House\u{F87B3}.", wh, NO_MODE),
    ];
    let shown = chat::Shown { rows: 8.0, draw_n: 8, pending: 0, slide: 0 };
    // A selection from the second line's middle into the fourth (chatsel.rs).
    *m.log_sel.borrow_mut() = Some(super::chatsel::LogSel { anchor: (1, 14), head: (3, 22) });
    let mut unread = [0; 11];
    unread[2] = 3;
    let named = |l: &str, mode: u16| mode == 13 && chat::mentions(l, "Lenon");
    chat::draw(&mut p, &ChatView { lines: lines.iter().enumerate().map(|(k, (a, b, c, d))| (a.clone(), *b, *c, *d, named(b, *d), k)).collect(), input: Some(InputView { channel: "Party", sel: Some((0, 7)), pieces: vec![("Pulling now ".into(), false), ("Thank you.".into(), true)], caret: 22, fep: None, preedit: String::new() }), waiting: false, unread }, shown, true);
    let fx = |icon, label: Option<&str>, ending| Effect { icon: Icon::Pack(icon), label: label.map(str::to_string), ending, tip: None };
    let mine = vec![fx("icons/protect", Some("25m"), false), fx("icons/haste", Some("2m"), false), fx("icons/refresh", Some("12"), true), fx("icons/poison", None, false)];
    hud::status_panel(&mut p, &StatusView { effects: mine, cursor: None }, false);
    let dot = |x, y, c: u32| (x, y, super::chat::colour(c.to_le_bytes()));
    let compass = hud::CompassView { heading: 0.6, dots: vec![dot(4.0, -9.0, 0x8030_60e0), dot(-11.0, 3.0, 0x8000_b040), dot(14.0, 13.0, 0x5000_b040)], range: 28.8, arrow: Some((0.6, -0.8, "focus")) };
    hud::where_panel(&mut p, &WhereView { zone: "West Ronfaure".into(), grid: "(F-9)".into(), time: (18, 42), day: (3, "Windsday".into()), weather: Some(11), compass: Some(compass), clock: Some(super::super::clock::Parts { earth: "20:40".into(), moon: ("Last Quarter Moon".into(), 60), next: ("Lightningday".into(), 46) }) });
    let theirs = vec![fx("icons/protect", None, false), fx("icons/sleep", None, false)];
    let vit = |hp: u32, mp: u32, f: f32| hud::RowVitals { hp, hp_max: Some(1000), mp, tp: Some(if f > 0.9 { 1250 } else { 300 }), hpf: f, mpf: Some(f * 0.8), hp_colour: if f < 0.75 { egui::Color32::from_rgb(0xff, 0xd0, 0x40) } else { egui::Color32::WHITE } };
    let row = |slot, name: &str, job, here: bool, hp: f32, marked, effects: Vec<Effect>| PartyRow { slot, name: name.into(), icon: hud::job_icon(job), sub_icon: hud::job_icon(if job == 3 { 5 } else { 0 }), here: here.then(|| vit((hp * 1000.0) as u32, 420, hp)), zone: "Port Jeuno".into(), marked, hand: marked.then_some(0x8080_8080), marks: (slot == 0) as u8, effects, jobs: String::new() };
    hud::party_panel(&mut p, "party", "Party", &[row(0, "Lenon", 5, true, 0.98, false, vec![]), row(1, "Raven", 1, true, 0.88, false, theirs.clone()), row(2, "Koru", 3, true, 0.62, true, vec![]), row(3, "Shantotto", 4, true, 0.95, false, vec![]), row(4, "Prishe", 2, false, 1.0, false, vec![])], false, Some(super::super::resttick::Shown::Next(0.6, 4.0)));
    // The two alliance parties as party_rows builds them: no MP bar, leader marks (bit 1 the alliance leader's), a
    // member out of the zone, a trust's row without TP.
    let ally = |slot, name: &str, job, here: bool, hp: f32, marks: u8, tp: bool| {
        let mut r = row(slot, name, job, here, hp, false, vec![]);
        if let Some(v) = r.here.as_mut() {
            v.mpf = None;
            v.tp = v.tp.filter(|_| tp);
        }
        PartyRow { marks, ..r }
    };
    if opts.alliance {
        let a1 = [ally(6, "Ayame", 12, true, 1.0, 3, true), ally(7, "Naji", 1, true, 0.42, 0, true), ally(8, "Kupipi", 3, true, 0.9, 0, false), ally(9, "Mihli Aliapoh", 3, false, 1.0, 0, true), ally(10, "Excenmille", 7, true, 0.15, 0, true), ally(11, "Trion", 7, true, 0.77, 0, true)];
        let a2 = [ally(12, "Zeid", 8, true, 0.66, 1, true), ally(13, "Lion", 6, true, 1.0, 0, true)];
        hud::party_panel(&mut p, "alliance1", "Alliance", &a1, false, None);
        hud::party_panel(&mut p, "alliance2", "Alliance", &a2, false, None);
    }
    hud::cast_panel(&mut p, "cast", "Casting", 0.6, "60%");
    // A refused action's chip over the cast bar (pending.rs).
    super::pending::panel(&mut p, &super::pending::View { what: "Cure III".into(), on: Some("Raven".into()), state: super::super::pending::State::Refused("Raven is out of range.".into()), secs: 0.6 });
if !opts.titles {
        hud::target_panel(&mut p, &TargetView { name: "Robber Crab".into(), ring: "indicators/target_hostile", locked: true, hp: Some((72, 0.72)), colour: Some(egui::Color32::from_rgb(255, 80, 80)), cast: Some(("Firaga".into(), Some(0.4))), distance: Some(12.4) });
    }
    if opts.titles {
        titles_sample(&mut p);
    }
    if opts.list {
        list_sample(&mut p);
    }
    if opts.kit {
        kit_sample(&mut p);
    }
    if opts.modes {
        let mut cfg = crate::config::ModernModes::default();
        cfg.pinned.push("party".into());
        super::modes::editor(&mut p, &mut true, &mut cfg, super::modes::Mode::Combat);
    }
    if opts.favs {
        favs_sample(&mut p);
    }
    if opts.gm {
        super::gmwin::sample(&mut p);
    }
    if opts.settings {
        let s = super::super::settings::SettingsUi::open(&crate::config::Config::default(), [1920, 1080]);
        super::config::sample(&mut p, &s);
    }
    if opts.menus {
        let r = p.this.get("party").copied().unwrap_or(Rect::NOTHING);
        super::ctxmenu::popup(ctx, m, pos2(r.min.x - 560.0, r.min.y - 40.0), None, &[("Use", true), ("Drop", true), ("Sort", true), ("Move to...", true)]);
        super::ctxmenu::popup(ctx, m, pos2(r.min.x - 380.0, r.min.y - 40.0), Some("Raven"), &[("Target", true), ("Check", true), ("Tell", true), ("Focus Target", true)]);
        super::ctxmenu::popup(ctx, m, pos2(r.min.x - 200.0, r.min.y - 40.0), Some("Robber Crab"), &[("Target", true), ("Attack", true), ("Check", true)]);
    }
    if opts.rows.is_some() || opts.borders.is_some() {
        menu_sample(&mut p);
    }
    *m.rects.borrow_mut() = std::mem::take(&mut p.this);
    if opts.borders.is_some() {
        edge_sample(m, ctx);
        skillchain_sample(m, ctx);
        let screen = ctx.content_rect();
        timers_sample(m, ctx, screen.center().x - 190.0 - super::extras::WIDTH, super::extras::WIDTH, super::extras::Side::Left);
        timers_sample(m, ctx, screen.center().x - 180.0, 360.0, super::extras::Side::Left);
    }
    if opts.list || opts.kit || opts.favs || opts.modes || opts.settings || opts.gm || opts.rows.is_some() || opts.borders.is_some() {
        return;
    }
    // Every frame at a few sizes: nine-slice corners, tiled edges, the stretched gradient.
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::unique("frames")));
    let screen = ctx.content_rect();
    let names = ["window_normal", "window_focused", "window_inactive", "window_alert", "window_tooltip"];
    let sizes = [vec2(90.0, 70.0), vec2(170.0, 110.0), vec2(260.0, 150.0)];
    let mut x = screen.center().x - 300.0;
    for (i, n) in names.iter().enumerate() {
        let Some(f) = m.art.frame(n) else { continue };
        let s = sizes[i % sizes.len()];
        let y = screen.center().y - s.y;
        painter.add(paint::window(f, Rect::from_min_size(pos2(x, y), s), 1.0, 1.0));
        painter.text(pos2(x + 12.0, y + 14.0), egui::Align2::LEFT_TOP, *n, egui::FontId::proportional(12.0), m.theme.color("muted"));
        x += s.x + 12.0;
    }
}

/// The shot's options: the chat's font size; another body and title face (regular, strong), which also puts a list
/// panel in place of the frames.
pub struct Opts {
    pub chat_font: f32,
    pub font: Option<(std::path::PathBuf, std::path::PathBuf)>,
    pub list: bool,
    /// A menu row style by name (rows.rs): the main menu and a submenu drawn in it.
    pub rows: Option<String>,
    /// A window border style by name (borders.rs): the panels, the menu and an edge window drawn in it.
    pub borders: Option<String>,
    /// The windows' background opacity.
    pub opacity: f32,
    /// A panel of the kit's controls.
    pub kit: bool,
    /// The UI Modes editor.
    pub modes: bool,
    /// The Settings window (config.rs).
    pub settings: bool,
    /// Ours: the GM window (gmwin.rs) with a level 5 sample.
    pub gm: bool,
    /// The favourites strip and starred list rows.
    pub favs: bool,
    /// The right-click menus beside the party window.
    pub menus: bool,
    /// The alliance windows with sample rows.
    pub alliance: bool,
    /// Title rows with long notes.
    pub titles: bool,
}

pub fn shot(width: u32, height: u32, scale: u32, backdrop: Option<&Path>, out: &Path, opts: &Opts) -> Result<()> {
    let gpu = Gpu::headless()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("modern shot"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let ctx = egui::Context::default();
    // The atlases load at once: the context learns the GPU's texture limit first (egui assumes 2048 until told).
    let side = Some(gpu.device.limits().max_texture_dimension_2d as usize);
    let mut warm = ctx.run_ui(egui::RawInput { max_texture_side: side, ..Default::default() }, |_| {});
    let mut deltas: Vec<_> = warm.textures_delta.set.drain().collect();
    warm.textures_delta.clear();
    let m = Modern::new(&ctx);
    m.set_opacity(opts.opacity);
    if let Some(name) = &opts.rows {
        m.row_style.set(super::rows::Style::from_name(name).with_context(|| format!("no row style {name}"))?);
    }
    if let Some(name) = &opts.borders {
        m.border_style.set(super::borders::Style::from_name(name).with_context(|| format!("no border style {name}"))?);
    }
    if let Some((r, s)) = &opts.font {
        super::theme::install_fonts_with(&ctx, Some((std::fs::read(r)?, std::fs::read(s)?)));
    }
    let zoom = m.theme.zoom(height as f32, scale);
    ctx.set_zoom_factor(zoom);
    let bg = backdrop.map(load_png).transpose()?.map(|(size, rgba)| ctx.load_texture("backdrop", egui::ColorImage::from_rgba_unmultiplied(size, &rgba), egui::TextureOptions::LINEAR));
    let mut layout = Layout::new();
    let mut last = None;
    // Frame 0 only sets the screen (egui's first frame has its default one); the time steps finish the fades.
    for frame in 0..8 {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width as f32, height as f32) / zoom)), time: Some(frame as f64 * 0.5), max_texture_side: side, ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx();
            if frame == 0 {
                return;
            }
            if let Some(t) = &bg {
                ctx.layer_painter(LayerId::background()).image(t.id(), ctx.content_rect(), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            sample(&m, ctx, &mut layout, opts);
        });
        // Taken as they come (a TexturesDelta dropped unapplied is an error in debug builds).
        deltas.extend(output.textures_delta.set.drain());
        output.textures_delta.clear();
        last = Some(output);
    }
    let output = last.context("no egui output")?;
    let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
    let mut renderer = egui_wgpu::Renderer::new(&gpu.device, format, Default::default());
    for (id, ds) in &deltas {
        for d in ds {
            renderer.update_texture(&gpu.device, &gpu.queue, *id, d);
        }
    }
    let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [width, height], pixels_per_point: output.pixels_per_point };
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    let extra = renderer.update_buffers(&gpu.device, &gpu.queue, &mut enc, &jobs, &screen);
    {
        let mut pass = enc
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("modern shot"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.04, g: 0.07, b: 0.09, a: 1.0 }), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();
        renderer.render(&mut pass, &jobs, &screen);
    }
    let row = (width * 4).div_ceil(256) * 256;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * height) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    enc.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(height) } },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    gpu.queue.submit(extra.into_iter().chain([enc.finish()]));
    super::super::save_capture(&gpu, &buf, [width, height], row, format, out, None, None)?;
    println!("wrote {}", out.display());
    Ok(())
}
