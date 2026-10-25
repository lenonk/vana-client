//! The windowed client: login and character select (in place of PlayOnline), then the world.

pub mod ground;
pub mod combat;
mod actmotion;
mod flyover;
mod creator;
pub use creator::{ScreenShot as CreatorScreenShot, screen_shot as creator_screen_shot};
pub use hudtop::topbar_shot;
mod event;
mod buffs;
mod game_ui;
mod hudtop;
mod augment;
mod iteminfo;
mod map;
mod mapmark;
mod widescan;
mod logwin;
pub(crate) mod perf;
mod cfgapply;
mod flash;
mod footsteps;
mod configui;
mod configui2;
mod settings;
mod updater;
mod firstrun;
mod desktop;
mod target;
mod input;
mod camera;
mod items;
pub mod select;
pub mod net;
pub mod script;
mod subarea;
mod myroom;
mod bump;
mod doors;
mod lifts;
mod mousecur;
mod party;
mod macros;
mod textcmd;
mod magic;
mod moogle;
mod post;
mod synth;
mod treasure;
mod servmes;
mod chatctrl;
mod linklist;
mod blklist;
mod furniture;
mod tilt;
mod aepoints;
mod jobpoints;
mod merits;
mod plate;
mod partymenu;
mod partyreq;
mod bluset;
mod roe;
mod levelsync;
mod viewhouse;
mod currencies;
mod unity;
mod bazaar;
mod helpdesk;
mod macroedit;
mod equipsets;
mod esmenu;
mod esedit;
mod faqmain;
mod equipwin;
mod journal;
mod missions;
mod quests;
mod keyitems;
mod linkshell;
mod searchmenu;
mod arealist;
mod seacom;
mod region;
mod radar;
mod ride;
mod status;
mod inspect;
mod linehist;
mod scrollbar;
mod search;
mod shop;
mod gauge;
mod trade;
mod fep;

use crate::config::Config;
use crate::dat::character::{Character as Model, Look};
use crate::dat::{DatIndex, zone};
use crate::net::auth::{self, Command, Reply, Request};
use crate::net::lobby::{Character, Lobby, MapLogin, NewCharacter};
use crate::render::{Camera, Gpu, ZoneRenderer, depth_view};
use anyhow::Result;
use glam::{Vec3, Vec4Swizzles};
use game_ui::{GREY, GameUiData, GameUi, WHITE, YELLOW};
use net::{EntityKind, NetCommand, NetEvent, NetHandle};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

const RACES: [&str; 8] = ["Hume (M)", "Hume (F)", "Elvaan (M)", "Elvaan (F)", "Tarutaru (M)", "Tarutaru (F)", "Mithra", "Galka"];
const JOBS: [&str; 6] = ["Warrior", "Monk", "White Mage", "Black Mage", "Red Mage", "Thief"];

const RUN_SPEED: f32 = 5.0;

impl Task {
    fn name(&self) -> &'static str {
        match self {
            Task::LoggedIn(_) => "logged in",
            Task::AccountCreated(_) => "account",
            Task::Lobby(_) => "lobby",
            Task::Selected(_) => "selected",
            Task::ZoneLoaded(..) => "zone loaded",
            Task::AreaLoaded(..) => "area loaded",
            Task::SubAreaLoaded(..) => "sub-area loaded",
            Task::CharacterLoaded(..) => "model loaded",
            Task::GameUi(_) => "game ui",
            Task::Flyover(_) => "flyover",
            Task::Nation(..) => "nation",
            Task::Select(..) => "select",
            Task::CreatorBackdrop(_) => "backdrop",
            Task::Dancer(_) => "dancer",
            Task::Battle(_) => "battle text",
            Task::Names(_) => "names",
            Task::Actions(_) => "actions",
            Task::Furniture(_) => "furniture",
        }
    }
}

enum Task {
    LoggedIn(Result<(Lobby, Vec<Character>)>),
    AccountCreated(Result<()>),
    Lobby(Result<(Lobby, Vec<Character>)>),
    Selected(Result<MapLogin>),
    /// Zone number, scene (map number) and the scene off the loader thread.
    ZoneLoaded(u16, u16, Result<(zone::ZoneScene, ground::Ground, Option<crate::dat::weather::ZoneWeather>, Arc<crate::render::ZoneGeometry>, ZoneEffects)>),
    AreaLoaded(u16, Result<(zone::ZoneScene, ground::Ground, Option<crate::dat::weather::ZoneWeather>, Arc<crate::render::ZoneGeometry>, Option<crate::fx::lib::Library>)>),
    /// A sub-area (zone, id) off the loader thread.
    SubAreaLoaded(u16, u32, Result<subarea::Loaded>),
    /// A model by its source key, prepared for the GPU on the loader thread.
    CharacterLoaded(String, Result<Model>),
    GameUi(Result<GameUiData>),
    Flyover(Result<flyover::FlyoverData>),
    /// A nation step flyover (file 24 + nation) has loaded.
    Nation(usize, Result<flyover::FlyoverData>),
    Select(Result<select::SelectData>, String),
    CreatorBackdrop(Result<creator::Backdrop>),
    Dancer(Result<creator::Dancer>),
    Battle(Result<crate::dat::battle::BattleText>),
    /// Item / key item names and the server text id map (checked against the install).
    Names(Result<(Arc<crate::dat::names::Names>, crate::dat::msgshift::MsgShift)>),
    /// Spell and ability records (file 81).
    Actions(Result<crate::dat::actions::Actions>),
    /// Mog House furniture geometry (client/furniture.rs).
    Furniture(Result<furniture::Loaded>),
}

/// A pick read-back key: bit 63, the actor, a bone or 0x8000 | attach point.
fn pick_key(id: u32, k: u16) -> u64 {
    1 << 63 | (id as u64) << 16 | k as u64
}

/// The main menu's disabled children on the current page (state 5; FUN_10140090 / FUN_10140270): Party while the party
/// is us and trusts only (FUN_10201f00). Not done: Monstrosity.
fn menu_disabled(w: &World) -> Vec<u16> {
    let mut off = Vec::new();
    if w.menu_page == 0 && w.party.slots().trusts_only() {
        off.push(6);
    }
    // FUN_10140270: row 12 (Mog House) is disabled while visiting another's house.
    if w.menu_page == 0 && w.myroom.is_some() && w.mh_owner != 0 {
        off.push(12);
    }
    off
}

/// FUN_1013ff90: in the Mog House row 12 reads "MogHouse" (windowps 44; 443 dim while visiting another's house) instead
/// of the layout's "Map".
fn menu_swap(w: &World) -> Vec<(u16, usize)> {
    if w.menu_page == 0 && w.myroom.is_some() { vec![(12, if w.mh_owner == 0 { 44 } else { 443 })] } else { Vec::new() }
}

/// The text engine has the keyboard (FUN_101adbd0: mode 4 the chat line, 5 a window's field): key functions with mask
/// 0x41 and movement stay off, as while typing in the chat line.
fn text_field_open(w: &World) -> bool {
    w.typing
        || event::input(w).is_some()
        || w.map.as_ref().is_some_and(|m| m.typing())
        || post::typing(w)
        || searchmenu::typing(w)
        || linkshell::typing(w)
        || seacom::typing(w)
        || macroedit::typing(w)
        || faqmain::typing(w)
}

/// A window that has the focus or blocks targeting (FUN_101585f0's T+0x74 bit 0 / T+0x6a; ours the open windows).
fn world_window_open(w: &World) -> bool {
    w.action_menu.is_some() || window_over_actions(w)
}

/// The open windows but the action menu (playermo), which stays up while engaged: macros run while it is the only one
/// (FUN_101a33e0 lets the focus be playermo, mp_pmode or a palette).
fn window_over_actions(w: &World) -> bool {
    w.ko_open || w.ko_offer == 2 || w.menu.is_some() || w.confirm.is_some() || w.win.is_some() || w.config.is_some() || w.settings.is_some() || w.map.is_some() || w.full_log.is_some() || w.magic.is_some() || w.ability.is_some() || w.cats.is_some() || w.moogle.is_some() || post::active(w) || synth::active(w) || shop::active(w) || trade::active(w) || w.search.is_some() || w.inspect.up() || w.mogdoor.is_some() || treasure::active(w) || w.chatctrl.is_some() || w.partymenu.is_some() || w.levelsync.is_some() || w.viewhouse.is_some() || w.bazaar.is_some() || w.helpdesk.is_some() || w.macroedit.is_some() || w.status.is_some() || w.missions.is_some() || w.quests.is_some() || w.keyitems.is_some() || w.searchmenu.is_some() || w.linkshell.is_some() || w.arealist.is_some() || w.region_menu.is_some() || w.cnq.is_some() || w.blklist.is_some()
}

/// FUN_100c63d0: a jump starts when none runs (draw actor +0x8bc = 32 ticks).
fn start_jump(w: &mut World, id: u32) {
    if let Some(a) = w.actors.get_mut(&id)
        && a.jump < 0.0
    {
        a.jump = 32.0;
    }
}

/// /jump (FUN_1007c4f0 -> FUN_1008b410): our jump and c2s 0x11D {UniqueNo, ActIndex}.
fn jump(w: &mut World) {
    let me = w.my_id;
    start_jump(w, me);
    let mut b = vec![0u8; 8];
    b[0..4].copy_from_slice(&w.my_id.to_le_bytes());
    b[4..6].copy_from_slice(&w.my_index.to_le_bytes());
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x11D, body: b });
}

/// FUN_10096640 / FUN_100882d0: c2s 0x01A ActionID 0x14 once per zone load (LSB SendResRdy; in the Mog House it
/// sends the moogle). Waits for the world (and our model), then outside the Mog House for SendCount KingFlag
/// actors or 600 ticks from the zone load; in the Mog House for the room object's 20-frame count.
#[derive(Default)]
struct ResRdy {
    pub sent: bool,
    send_count: u16,
    kings: HashSet<u32>,
    since: Option<Instant>,
}

fn res_rdy(w: &mut World) {
    if w.res_rdy.sent || !event::zone_ready(w) {
        return;
    }
    let since = *w.res_rdy.since.get_or_insert_with(Instant::now);
    let ticks = since.elapsed().as_secs_f32() * 60.0;
    // Ticks of 1/60 s for the room's 20 frames (inferred).
    let go = if w.myroom.is_some() { ticks >= 20.0 } else { w.res_rdy.kings.len() >= w.res_rdy.send_count as usize || ticks >= 600.0 };
    if go {
        w.res_rdy.sent = true;
        let body = net::action_body(w.my_id, w.my_index, 0x14);
        let _ = w.net.commands.send(NetCommand::Packet { id: 0x01A, body });
        log::info!("0x01A SendResRdy sent");
    }
}

struct Entity {
    name: String,
    pos: Vec3,
    kind: EntityKind,
    hpp: u8,
    dir: u8,
    /// Where the server last put it; `pos` moves there at the entity's speed.
    target: Vec3,
    motion: net::MotionState,
    /// Smoothed movement ratio (FUN_100c68f0) and the position drawn last frame.
    ratio: f32,
    last_pos: Vec3,
    /// Who claimed it (0 = nobody), and whether it is a monster.
    claim: u32,
    mob: bool,
    /// ActIndex (targid), for 0x01A and 0x0DD.
    index: u16,
    /// Name flags (u32 @0x24 bits 29 / 30): no article, plural verbs.
    named: bool,
    plural: bool,
    /// FUN_1008b680 slide toward `target`: its position, ticks left, the last 0x00E counter; the drawn yaw
    /// (radians, forward = (cos, 0, -sin)) and the heading it was set from.
    slide: Vec3,
    remain: f32,
    counter: u16,
    yaw: f32,
    dir_seen: u8,
    /// Snapped to the collision floor every frame (0x00E: NPC id, u32 @0x18 bit 15 clear, kind not 2 / 3).
    grounded: bool,
    /// Status bits u8 @0x20 & 7 (actor +0x120 bits 13 / 14 / 18): bit 1 hides it outside events, bits 1 / 2
    /// make it untargetable (FUN_1008cdc0). LSB's "cutscene only" status 6 sets both.
    flags: u8,
    /// Actor +0x130 bit 20 and +0x128 bit 3: no target gauge (FUN_10149900); bit 3 also no name plate
    /// (FUN_10082ec0). 0x00D clears bit 3, so players have both.
    no_gauge: bool,
    no_plate: bool,
    /// Entity +0x130 bit 1 (scheduler op 0x9a): the model file's 0x1000 generators don't start.
    gens_off: bool,
    /// Actor +0x130 bit 7 (0x00E u8 @0x2B bit 0x08).
    untargetable: bool,
    /// client/radar.rs: InvisFlag (+0x128 bit 6), 0x00E @0x2B bit 0x40 (+0x130 bit 15), the Monstrosity word (+0x210).
    invisible: bool,
    radar_off: bool,
    monstrosity: u16,
    /// The name plate's other fields (0x00D @0x32 / @0x33) and 0x067 @0x10 (Campaign bit 1, Level Sync bit 2).
    plate: net::PlateExtra,
    sync: u8,
    /// +0x124: the sub-animation wanted (bits 1..3), applied (bits 4..7, 8 = none yet), and the model's own (+0x8a5).
    sub: [u8; 3],
    /// +0x2a0: the master's ActIndex (s2c 0x067 kind 3 @0x0C; LSB sends it for our trusts), 0 none.
    master: u16,
    /// +0x128 bit 29: the model's "init" ran (FUN_1008c480).
    init_done: bool,
    /// A door's id (0x00E SubKind 2, u32 @0x34); 0 for anything else.
    door: u32,
    /// A lift's (shaft id, move start, move end) (0x00E SubKind 3, client/lifts.rs).
    lift: Option<(u32, u32, u32)>,
    /// client/bump.rs: ticks left of "active" (+0x5ae), made passable by event op 0xab 0x0b (+0x120 bit 6), and the
    /// SubKind 0 model id (+0xfc).
    active: f32,
    no_bump: bool,
    model: u16,
    /// An object (actor +0x128 bit 3 from the 0x00E look): no target gauge, no name plate.
    object: bool,
    /// The ActIndex its head looks at (0x00E / 0x00D facetarget, actor +0x1f8); 0 = none.
    face: u16,
    /// Actor +0x12c bit 23 (0x00E / 0x00D @0x28 bit 0x04): a new model plays 'spop' (FUN_1008f740).
    spawn_fx: bool,
    /// 0x00D's riding fields (ride.rs).
    ride: net::Ride,
    /// Entity +0x1d8: an "menu    ncol" index event op 0xae 1 forces the name colour to, 0x17 none (op 0xae 2).
    name_ncol: u16,
    /// Name-colour marks (net::Marks) and SubKind 2..4 (doors, lifts, ships); a female PC (0x00D @0x20 bit 15 clear).
    marks: net::Marks,
    female: bool,
    sub_kind: u8,
}

impl Entity {
    fn targetable(&self) -> bool {
        self.flags & 6 == 0
    }
}

/// Screens of the lobby after login, reached from the title menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LobbyScreen {
    Main,
    Select,
    Create,
    Delete,
    Config,
}

/// Lobby screen changes (FUN_10233d50 / FUN_10233cf0, schedulers of file 33193): 'sfot' + 'ffot' take the 2D layer
/// and the scene to black over 60 ticks; the next screen starts 34 frames after the fade began (FUN_1023e5b0: 68 ticks
/// at the lobby's 30 fps), and 'sfin' + 'ffin' bring both back over 60 ticks, with 8 frames (16 ticks) before the new
/// screen takes input (FUN_1023e550). The creator brings in only its 2D layer (FUN_10233db0); its scene follows once
/// the dancer shows (FUN_10245fb0).
struct LobbyFade {
    ui: f32,
    scene: f32,
    ui_to: f32,
    scene_to: f32,
    /// Ticks since the fade-out began, and the ticks left before the faded-in screen takes input.
    since: f32,
    wait: f32,
    next: Option<Box<dyn FnOnce(&mut App)>>,
}

impl Default for LobbyFade {
    fn default() -> Self {
        LobbyFade { ui: 0.0, scene: 0.0, ui_to: 1.0, scene_to: 1.0, since: 0.0, wait: 0.0, next: None }
    }
}

impl LobbyFade {
    /// Fades out, then runs `next` (a change asked for meanwhile is dropped).
    fn out(&mut self, next: Box<dyn FnOnce(&mut App)>) {
        if self.next.is_some() {
            return;
        }
        (self.ui_to, self.scene_to, self.since, self.next) = (0.0, 0.0, 0.0, Some(next));
    }

    /// The next screen fades in; the creator's scene waits for its dancer.
    fn enter(&mut self, scene: bool) {
        (self.ui_to, self.scene_to, self.wait) = (1.0, if scene { 1.0 } else { 0.0 }, 16.0);
    }

    /// Screen changes wait for the fade-out and for the fade-in's first 8 frames.
    fn busy(&self) -> bool {
        self.next.is_some() || self.wait > 0.0
    }

    /// One frame of `ticks`; returns the change to make 68 ticks after the fade-out began.
    fn step(&mut self, ticks: f32) -> Option<Box<dyn FnOnce(&mut App)>> {
        let to = |v: f32, t: f32| if v < t { (v + ticks / 60.0).min(t) } else { (v - ticks / 60.0).max(t) };
        self.ui = to(self.ui, self.ui_to);
        self.scene = to(self.scene, self.scene_to);
        self.wait = (self.wait - ticks).max(0.0);
        if self.next.is_some() {
            self.since += ticks;
            if self.since >= 68.0 {
                return self.next.take();
            }
        }
        None
    }
}

/// Pointer and keyboard input for the lobby screens, in physical pixels.
struct UiInput {
    pointer: Option<(f32, f32)>,
    moved: bool,
    clicked: bool,
    /// The left button's press edge, held state and release edge (a swallowed click has none); a right click.
    pressed: bool,
    down: bool,
    released: bool,
    right_clicked: bool,
    /// Wheel notches for the focused list, up positive (FUN_10125360).
    wheel: i32,
    events: Vec<egui::Event>,
}

fn inside(r: &[f32; 4], p: (f32, f32)) -> bool {
    p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3]
}

/// Fills the title's text slots: the help line and the version.
fn lobby_text(lu: &mut GameUi, slots: &[crate::render::screens::TextSlot], help: &str, color: [u8; 4], version: &str) {
    for t in slots {
        match t.what {
            "help line" => lu.text(help, t.x, t.y, t.align, color),
            w if w.starts_with("ver.") => lu.text(&format!("ver.{version}"), t.x, t.y, t.align, WHITE),
            _ => {}
        }
    }
}

const LOGIN_LABELS: [&str; 4] = ["Server", "Account", "Password", "One-time code"];

/// The tail of `text` that fits in `max` pixels, so the caret end stays visible.
fn fit_tail(lu: &GameUi, text: &str, max: f32) -> String {
    let mut t = text;
    while lu.text_width(t) > max {
        let mut c = t.chars();
        c.next();
        t = c.as_str();
    }
    t.to_string()
}

/// What the credentials window's actions do.
#[derive(Clone, Copy, PartialEq)]
enum LoginChoice {
    LogIn,
    Create,
    Exit,
}

/// The credentials window over the title art (ours: retail logged in through PlayOnline). Focus 0..3 are the
/// fields, 4..6 Log in / Create account / Exit.
#[allow(clippy::too_many_arguments)]
/// Not retail (PlayOnline logged in): the generic window sounds (FUN_10118260) go to `sounds`: 0001 when the focus
/// moves (and on Escape, which lands on Exit), 0002 on an enabled action, 0004 on a disabled one.
#[allow(clippy::too_many_arguments)]
fn login_window(lu: &mut GameUi, input: &UiInput, size: [u32; 2], fields: [&mut String; 4], focus: &mut usize, busy: bool, status: &str, version: &str, sounds: &mut Vec<crate::audio::Ui>) -> Option<LoginChoice> {
    let was = *focus;
    const CHOICES: [LoginChoice; 3] = [LoginChoice::LogIn, LoginChoice::Create, LoginChoice::Exit];
    const ITEMS: usize = 7;
    let (sw, sh) = (size[0] as f32, size[1] as f32);
    let (ww, wh) = (380.0, 140.0);
    let (wx, wy) = (((sw - ww) / 2.0).round(), (sh * 440.0 / 768.0).round());
    let row = |i: usize| wy + 14.0 + i as f32 * 22.0;
    let field_rect = |i: usize| [wx + 148.0, row(i), ww - 164.0, 18.0];
    let ay = row(4) + 6.0;
    // Config-page buttons (88 px, wider for a longer caption), centred 24 px apart.
    const ACTIONS: [&str; 3] = ["Log in", "Create account", "Exit"];
    let widths = ACTIONS.map(|a| (lu.label_width(a) + 14.0).max(88.0).round());
    let mut bx = (wx + (ww - widths.iter().sum::<f32>() - 48.0) / 2.0).round();
    let arects = [0, 1, 2].map(|k| {
        let r = [bx, ay, widths[k], 16.0];
        bx += widths[k] + 24.0;
        r
    });
    // Log in and Create account need the server, account and password.
    let filled = !fields[0].trim().is_empty() && !fields[1].trim().is_empty() && !fields[2].is_empty();
    let enabled = [filled, filled, true];

    let mut chosen = None;
    // Hovering an action moves the selection there, as in the game's menus.
    if let Some(k) = input.pointer.and_then(|p| (0..3).find(|&k| inside(&arects[k], p))) {
        lu.hover.set(true);
        if input.moved {
            *focus = 4 + k;
        }
    }
    if input.clicked
        && let Some(p) = input.pointer
    {
        if let Some(i) = (0..4).find(|&i| inside(&field_rect(i), p)) {
            *focus = i;
        }
        if let Some(k) = (0..3).find(|&k| inside(&arects[k], p)) {
            *focus = 4 + k;
            chosen = enabled[k].then_some(CHOICES[k]);
            sounds.push(if enabled[k] { crate::audio::Ui::Select } else { crate::audio::Ui::Error });
        }
    }
    let mut escape = false;
    for ev in &input.events {
        match ev {
            egui::Event::Text(t) | egui::Event::Paste(t) if *focus < 4 => {
                let f = &mut *fields[*focus];
                f.extend(t.chars().filter(|c| !c.is_control()));
                *f = f.chars().take(64).collect();
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => match key {
                egui::Key::Tab => *focus = if modifiers.shift { (*focus + ITEMS - 1) % ITEMS } else { (*focus + 1) % ITEMS },
                egui::Key::ArrowDown => *focus = (*focus + 1).min(4.max(*focus)),
                egui::Key::ArrowUp => *focus = if *focus >= 4 { 3 } else { focus.saturating_sub(1) },
                egui::Key::ArrowLeft if *focus > 4 => *focus -= 1,
                egui::Key::ArrowRight if (4..6).contains(&*focus) => *focus += 1,
                egui::Key::Backspace if *focus < 4 => {
                    fields[*focus].pop();
                }
                // Escape goes to Exit, as a menu's cancel lands on its back item.
                egui::Key::Escape => {
                    *focus = 6;
                    escape = true;
                }
                egui::Key::Enter => {
                    let k = focus.saturating_sub(4);
                    chosen = enabled[k].then_some(CHOICES[k]);
                    sounds.push(if enabled[k] { crate::audio::Ui::Select } else { crate::audio::Ui::Error });
                }
                _ => {}
            },
            _ => {}
        }
    }

    if (*focus != was || escape) && chosen.is_none() {
        sounds.push(crate::audio::Ui::Cursor);
    }
    let help = if busy {
        "Connecting to the server..."
    } else if !status.is_empty() {
        status
    } else if *focus >= 4 && !enabled[*focus - 4] {
        "Enter the server, account name and password first."
    } else {
        match *focus {
            4 => "Log in with this account.",
            5 => "Create a new account on this server with the account name and password above.",
            6 => "Exit the game.",
            _ => "Enter your account information to log in.",
        }
    };
    let slots = lu.title(sw, sh, None, 0, [true; 5]);
    lobby_text(lu, &slots, help, if busy || status.is_empty() { WHITE } else { YELLOW }, version);
    lu.frame(wx, wy, ww, wh);
    for (i, label) in LOGIN_LABELS.iter().enumerate() {
        let r = field_rect(i);
        lu.text(label, wx + 16.0, row(i) + 1.0, 0, WHITE);
        lu.rect(r[0], r[1], r[2], r[3], if *focus == i { [0.16, 0.2, 0.45, 0.6] } else { [0.0, 0.0, 0.0, 0.35] });
        let raw = if i == 2 { "*".repeat(fields[i].chars().count()) } else { fields[i].clone() };
        let shown = fit_tail(lu, &raw, r[2] - 10.0);
        if shown.is_empty() && i == 3 && *focus != 3 {
            lu.text("only with 2FA", r[0] + 4.0, r[1] + 1.0, 0, GREY);
        }
        lu.text(&shown, r[0] + 4.0, r[1] + 1.0, 0, WHITE);
        if *focus == i && lu.blink() {
            lu.rect(r[0] + 4.0 + lu.text_width(&shown), r[1] + 2.0, 2.0, 14.0, [1.0, 1.0, 1.0, 0.9]);
        }
    }
    // As the Config pages' buttons: the selected one gold (0x80805020) one pixel up-left, the hand over it last;
    // disabled grey.
    let mut hand = None;
    for (k, r) in arects.iter().enumerate() {
        let on = *focus == 4 + k && !busy;
        let d = if on { -1.0 } else { 0.0 };
        let tint = if busy || !enabled[k] { 0x8040_4040 } else if on { 0x8080_5020 } else { 0x8080_8080 };
        settings::button(lu, [r[0] + d, r[1] + d, r[2], r[3]], tint);
        let lx = r[0] + ((r[2] - lu.label_width(ACTIONS[k])) / 2.0).round();
        lu.label(ACTIONS[k], lx + d, r[1] + 1.0 + d, tint, true);
        if on {
            hand = Some((r[0] + d, r[1] + d));
        }
    }
    if let Some((hx, hy)) = hand {
        lu.cursor(hx, hy);
    }
    if busy { None } else { chosen }
}

/// A line from a menu or key (text command mode 0).
fn send_chat(world: &mut World, text: String) {
    textcmd::run(world, &text, textcmd::Mode::Internal);
}

/// c2s 0x01A on ourselves with a menu answer in StatusId (0x0D raise, 0x13 tractor; 0 accept, 1 reject).
fn send_menu_answer(w: &World, action: u16, status: u32) {
    let mut body = net::action_body(w.my_id, w.my_index, action);
    body[8..12].copy_from_slice(&status.to_le_bytes());
    let _ = w.net.commands.send(NetCommand::Packet { id: 0x01A, body });
}

/// LAB_10097370: the raise answer, then accepting closes the K.O. window and waits 20 s (mode 1, sub 4, 1200 ticks);
/// declining keeps it.
fn answer_raise(w: &mut World, yes: bool) {
    send_menu_answer(w, 0x0D, !yes as u32);
    w.ko_offer = 0;
    if yes {
        w.ko_open = false;
        w.ko_wait = Some(Instant::now() + std::time::Duration::from_secs(20));
    }
}

/// The game clock in Earth seconds since the Vana'diel epoch (FUN_101039e0: 0x00A GameTime plus the time since).
pub(super) fn vana_now(w: &World) -> u64 {
    w.clock.map_or(0, |(t, at)| t as u64 + at.elapsed().as_secs())
}

/// FUN_10096c90 mode 1 for us: dead, the K.O. window opens (closing the other windows, FUN_1015f3e0(0x800)) until
/// the deadline; "go home" chosen or the deadline passed (which closes the window) sends c2s 0x01A 0x0B, again every
/// 600 ticks while still dead. Alive again: everything goes. (Ours waits while no deadline is known yet.)
fn ko_step(w: &mut World) {
    if !w.dead() {
        // A raise prompt closed unanswered: decline it (FUN_10096640's state[2]).
        if w.ko_offer == 2 {
            send_menu_answer(w, 0x0D, 1);
        }
        (w.ko_open, w.homing, w.home_sent, w.ko_offer, w.ko_wait) = (false, false, None, 0, None);
        if matches!(w.confirm, Some((Confirm::HomePoint | Confirm::Tractor, _))) {
            w.confirm = None;
        }
        return;
    }
    // Mode 2 (the raise offer): comyn closes every frame, and the K.O. window comes back if it is down (FUN_1012e0c0).
    if w.ko_offer == 2 {
        w.confirm = None;
        if !w.ko_open {
            close_for_ko(w);
            w.ko_open = true;
        }
    }
    if w.ko_wait.is_some_and(|t| Instant::now() < t) {
        return;
    }
    w.ko_wait = None;
    let expired = w.ko_deadline != 0 && vana_now(w) >= w.ko_deadline;
    if expired && !w.homing {
        w.homing = true;
        w.ko_open = false;
        if matches!(w.confirm, Some((Confirm::HomePoint, _))) {
            w.confirm = None;
        }
    }
    if !w.homing && !w.ko_open && w.ko_deadline != 0 {
        close_for_ko(w);
        if !matches!(w.confirm, Some((Confirm::HomePoint, _))) {
            w.confirm = None;
        }
        w.ko_open = true;
    }
    if w.homing && w.home_sent.is_none_or(|t| t.elapsed().as_secs_f32() >= 10.0) {
        w.home_sent = Some(Instant::now());
        let _ = w.net.commands.send(NetCommand::HomePoint);
    }
}

/// FUN_1012e0c0's FUN_1015f3e0(0x800): every window but the logs, bars, party and map closes as the K.O. window opens.
fn close_for_ko(w: &mut World) {
    if w.prompt.is_some() {
        target::cancel_sub(w);
    }
    (w.menu, w.win, w.config, w.settings, w.action_menu, w.mogdoor) = (None, None, None, None, None, None);
    (w.magic, w.ability, w.cats) = (None, None, None);
}

/// A chat line with its timestamp (config 0xb5, "%s[%s]%s %s" at 0x1036344c).
/// An effect's sound command (fx/sound.rs) to the sound thread.
fn play_fx_sound(a: &mut crate::audio::Audio, c: crate::fx::sound::SoundCmd) {
    match c {
        crate::fx::sound::SoundCmd::Play { handle, se, vol, pan, stream } => {
            a.play_voice(handle, se, vol, pan, stream);
        }
        crate::fx::sound::SoundCmd::Level { handle, vol } => a.voice_level(handle, vol),
        crate::fx::sound::SoundCmd::Pan { handle, pan } => a.voice_pan(handle, pan),
        crate::fx::sound::SoundCmd::Stop { handle, ticks } => a.voice_stop(handle, ticks),
    }
}

/// 0x0B5 kind for a channel command (the 0x1037ade8 list plus /yell). Assist J / E: handlers FUN_10078be0 /
/// FUN_10078d80 send 0x22 / 0x23 *(the name to handler link inferred from the handler table order)*.
fn chat_channel(cmd: &str) -> Option<u8> {
    Some(match cmd {
        "/say" | "/s" => 0,
        "/shout" | "/sh" => 1,
        "/party" | "/p" => 4,
        "/linkshell" | "/l" => 5,
        "/yell" => 0x1a,
        "/linkshell2" | "/l2" => 0x1b,
        "/unity" | "/u" => 0x21,
        "/assistj" | "/aj" => 0x22,
        "/assiste" | "/ae" => 0x23,
        _ => return None,
    })
}

/// s2c 0x017 (LAB_100e37a0 -> FUN_100d9b20): the line and its log mode by Kind (byte table 0x100e3de8, jump table
/// 0x100e3da0); None for the kinds retail drops (2, 9..0xb, 0xc the GM prompt menu, past 0x23). Attr bit 0 adds
/// "[GM]"; the yell takes the zone's name; assist (0x22 / 0x23) the rank icon 0xEF (mentor x 10 + mastery + 0x24,
/// 0x2f..0x4c) when Data is set. Not done: the block list, config 0xce's spam filter, PvP (s2c 0x0B4) dropping the
/// linkshell's "[n]", the profanity mask, the line cut at a control byte.
fn chat_line(kind: u8, attr: u8, data: u16, from: &str, text: &str, zone: &str) -> Option<(String, u8)> {
    let gm = if attr & 1 != 0 { "[GM]" } else { "" };
    let n = from;
    let say = || format!("{gm}{n} : {text}");
    Some(match kind {
        0 | 0x18 => (say(), 9),
        0x19 if n.is_empty() => (text.to_string(), 9),
        0x19 => (say(), 9),
        1 => (say(), 10),
        3 => (format!("{gm}{n}>> {text}"), 0xc),
        4 => (format!("({gm}{n}) {text}"), 0xd),
        5 => (format!("[1]<{gm}{n}> {text}"), 0xe),
        0x1b => (format!("[2]<{gm}{n}> {text}"), 0xd6),
        0x1e => (format!("[3]<{gm}{n}> {text}"), 0x79),
        6 | 7 => (format!("----== SystemMessage ==----\n{text}"), 0xc8),
        8 => (format!("{gm}{n} {text}"), 0xf),
        0x1a => (format!("{gm}{n}[{zone}]: {text}"), 0xb),
        0x21 => (format!("{{{gm}{n}}} {text}"), 0xd4),
        0x22 | 0x23 => {
            let c = if kind == 0x22 { 'J' } else { 'E' };
            let [mastery, mentor] = data.to_le_bytes();
            let r = mentor.wrapping_mul(10).wrapping_add(mastery).wrapping_add(0x24);
            let rank = if data != 0 { crate::dat::font::decode_sjis(&[0xef, if (0x2f..=0x4c).contains(&r) { r } else { 0x2f }]) } else { String::new() };
            (format!("{gm}{rank}{n}({c}) : {text}"), if kind == 0x22 { 0xdc } else { 0xde })
        }
        // "NS" kinds: the text alone.
        0xd => (text.to_string(), 9),
        0xe => (text.to_string(), 10),
        0xf => (text.to_string(), 0xd),
        0x10 => (text.to_string(), 0xe),
        0x1c => (text.to_string(), 0xd6),
        0x11..=0x17 | 0x1d | 0x1f | 0x20 => (text.to_string(), 0x79),
        _ => return None,
    })
}

/// A chat line in its log mode's colour (the config colour before the message tables load), auto-translate blocks
/// expanded with their brackets (FUN_10127c00 with param 5).
/// FUN_10127990: a party line (modes 5 / 0x0d) with a call tag outside quotes plays its sound (the first tag that
/// matches; FUN_100380f0 plays one at a time). The tell sound (mode 0x0c, config 0x47) names SeSep "0p40", which no
/// resident file holds: not played.
fn call_sound(bt: &crate::dat::battle::BattleText, line: &str, mode: u8) -> Option<u32> {
    if !matches!(mode, 5 | 0x0d) {
        return None;
    }
    let b = line.as_bytes();
    let mut i = 0;
    while let Some(open) = b[i..].iter().position(|&c| c == b'<').map(|p| p + i) {
        let Some(close) = b[open..].iter().position(|&c| c == b'>').map(|p| p + open) else { break };
        let quoted = open > 0 && b[open - 1] == b'"' && b.get(close + 1) == Some(&b'"');
        if !quoted && let Some((_, n)) = bt.calls.iter().find(|(t, _)| line[open..].starts_with(t.as_str())) {
            return Some(*n);
        }
        i = open + 1;
    }
    None
}

fn push_chat_mode(world: &mut World, line: String, mode: u8, kind: u8) {
    if let Some(n) = world.battle.as_ref().and_then(|bt| call_sound(bt, &line, mode)) {
        world.sounds.push(n);
    }
    let (colour, line) = match world.battle.clone() {
        Some(bt) => {
            let names = world.names.clone();
            let line = bt.phrases.as_ref().map_or(line.clone(), |d| d.expand(&line, names.as_deref().map(|n| n as &dyn crate::dat::names::NameSource)));
            (bt.colour_in(mode, &world.game), line)
        }
        None => (cfgapply::chat_colour(&world.game, kind), line),
    };
    for l in line.split('\n') {
        world.chat.push_chat(l.to_string(), colour);
    }
}

/// Ctrl/Alt + S/T/P/L/U/X (LAB_101a64c0): open the input on a channel, keeping a message being typed
/// minus its own channel command (FUN_101a6550).
fn chat_prefix(world: &mut World, prefix: &str) {
    let typed = if world.typing { std::mem::take(&mut world.chat_input) } else { String::new() };
    let body = strip_channel(typed.trim_start());
    world.chat_input = format!("{prefix}{body}");
    (world.chat_caret, world.chat_anchor) = (None, None);
    world.typing = true;
}

fn strip_channel(s: &str) -> &str {
    let (cmd, rest) = s.split_once(' ').unwrap_or((s, ""));
    let cmd = cmd.to_ascii_lowercase();
    if cmd == "/tell" || cmd == "/t" {
        rest.split_once(' ').map_or("", |(_, m)| m)
    } else if chat_channel(&cmd).is_some_and(|k| k != 0x1a) {
        rest
    } else {
        s
    }
}

/// Ctrl/Alt + R (LAB_101a6820): "/tell <last sender> ", and again while typing for the one before.
fn reply(world: &mut World) {
    let typed = if world.typing { std::mem::take(&mut world.chat_input) } else { String::new() };
    let current = typed.strip_prefix("/tell ").and_then(|r| r.split(' ').next()).and_then(|n| world.tells.iter().position(|t| t == n));
    let next = current.map_or(0, |i| (i + 1) % world.tells.len().max(1));
    let body = strip_channel(typed.trim_start());
    world.chat_input = match world.tells.get(next) {
        Some(name) => format!("/tell {name} {body}"),
        None => format!("/tell {body}"),
    };
    world.chat_caret = None;
    world.typing = true;
}

/// Channel prefixes of key functions 0x3c, 0x38, 0x3a, 0x3e, 0xad, 0xae.
fn chat_shortcut(k: egui::Key) -> Option<&'static str> {
    Some(match k {
        egui::Key::S => "/say ",
        egui::Key::T => "/tell ",
        egui::Key::P => "/party ",
        egui::Key::L => "/linkshell ",
        egui::Key::U => "/unity ",
        egui::Key::X => "/linkshell2 ",
        _ => return None,
    })
}

/// Key function 0x78 (numpad +, Ctrl / Alt + F), FUN_10151550: with focus somewhere (or nowhere to go) back
/// to 0, else SE "0013" and the next of logwindo / buff (not modelled: no buff window) / none.
fn log_cycle(w: &mut World) {
    if w.full_log.is_some() || text_field_open(w) || event::active(w) {
        return;
    }
    // FUN_1015ecc0(0x20000 / 0x20000000) with no focus yet: an open window flagged in the registry (0x103722f8)
    // stops it: mgcmenu / abimenu / magselec / abiselec stay open under their lists, sortyn, ... The item windows'
    // share of this is inferred (Num+ is their list's message 8 there).
    if w.log_focus == 0 && (w.magic.is_some() || w.ability.is_some() || w.cats.is_some() || w.win.is_some() || w.confirm.is_some() || synth::active(w) || post::active(w) || shop::active(w) || trade::active(w) || treasure::active(w) || w.chatctrl.is_some() || w.partymenu.is_some() || w.levelsync.is_some() || w.viewhouse.is_some() || w.bazaar.is_some() || w.helpdesk.is_some() || w.macroedit.is_some() || w.status.is_some() || w.missions.is_some() || w.quests.is_some() || w.keyitems.is_some() || w.searchmenu.is_some() || w.linkshell.is_some() || w.arealist.is_some() || w.region_menu.is_some() || w.blklist.is_some()) {
        return;
    }
    let active = w.log_active || w.menu.is_some() || w.win.is_some() || w.action_menu.is_some();
    if w.log_focus != 0 && !active {
        w.log_focus = 0;
        return;
    }
    w.sounds.push(13);
    w.log_focus = (w.log_focus + 1) % 3;
    // The buff list takes the focus only with icons (FUN_10227bf0 gives it up at 0).
    if w.log_focus == 2 && !w.buffs.iter().any(|&b| b != 0xffff && b != 0) {
        w.log_focus = 0;
    }
    w.buff_cursor = 0;
    w.log_active = w.log_focus == 1;
}

/// The full log's keys (FUN_10133890): arrows move the row / scroll (SE "0001"), Enter on a speaker line
/// opens charlnk (not done; SE "0004"), Escape closes it and refocuses logwindo.
fn full_log_key(w: &mut World, key: egui::Key, held: bool, sh: f32) {
    let Some(fl) = w.full_log.as_mut() else { return };
    match key {
        egui::Key::Escape => {
            w.full_log = None;
            w.log_active = w.log_focus == 1;
            w.sounds.push(crate::audio::Ui::Cancel as u32);
        }
        egui::Key::Enter => w.sounds.push(4),
        k => {
            if fl.key(k, held, logwin::full_lines(sh), w.log.history()) {
                w.sounds.push(1);
            }
        }
    }
}

/// The full log's bar (its handler is not traced *(inf)*): a row is the scroll-only Up / Down, a page Left / Right,
/// the thumb the bottom-anchored top.
fn bar_full_log(w: &mut World, m: scrollbar::Msg, (total, lines, _): (usize, usize, usize)) -> bool {
    let Some(fl) = w.full_log.as_mut() else { return false };
    let key = match m {
        scrollbar::Msg::Up => egui::Key::ArrowUp,
        scrollbar::Msg::Down => egui::Key::ArrowDown,
        scrollbar::Msg::PageUp => egui::Key::ArrowLeft,
        scrollbar::Msg::PageDown => egui::Key::ArrowRight,
        scrollbar::Msg::Set(t) => {
            let s = total.saturating_sub(lines + t);
            let moved = s != fl.scroll;
            fl.scroll = s;
            return moved;
        }
    };
    fl.key(key, true, lines, total)
}

/// FUN_10130280 (Ctrl+C): talk to an NPC target, check anything else.
fn check_target(world: &mut World) {
    match world.target.filter(|t| t >> 24 != 0 && world.entities.get(t).is_some_and(|e| !e.mob)) {
        Some(t) => {
            event::talk(world, t);
        }
        None => {
            combat::choose(world, combat::cmd::CHECK);
        }
    }
}

/// Name colour index into "menu    ncol" (a subset of FUN_1008c430): players white, NPCs green,
/// monsters pale yellow, red when we claimed them, magenta when someone else did.
/// Ourselves as a target (retail keeps the player in the entity array): what the target window and the target code
/// read from an entity.
fn self_entity(w: &World) -> Entity {
    Entity {
        name: w.name.clone(),
        pos: w.pos,
        kind: EntityKind::Player,
        hpp: w.vitals.map_or(100, |v| v.3),
        dir: 0,
        target: w.pos,
        motion: w.my_motion,
        ratio: 0.0,
        last_pos: w.pos,
        claim: 0,
        mob: false,
        index: w.my_index,
        named: false,
        plural: false,
        slide: w.pos,
        remain: 0.0,
        counter: 0,
        yaw: w.yaw,
        dir_seen: 0,
        grounded: false,
        flags: 0,
        no_gauge: false,
        no_plate: false,
        gens_off: false,
        untargetable: false,
        invisible: false,
        radar_off: false,
        plate: net::PlateExtra::default(),
        sync: 0,
        monstrosity: 0,
        sub: [0, 8, 0],
        master: 0,
        init_done: false,
        door: 0,
        lift: None,
        active: 0.0,
        no_bump: false,
        model: 0,
        object: false,
        face: 0,
        spawn_fx: false,
        ride: Default::default(),
        name_ncol: 0x17,
        marks: net::Marks::default(),
        female: false,
        sub_kind: 0,
    }
}

/// FUN_1008db40: the name colour, an "menu    ncol" entry (`c`) or the alliance claim blend; first an event's forced
/// index (+0x1d8 below 0x17). Not done: PvP teams and ballista sides, the charmed-pet master's colour, the shared-claim
/// zone flag.
fn name_colour(id: u32, e: &Entity, w: &World, c: impl Fn(usize) -> u32) -> u32 {
    if e.name_ncol < 0x17 {
        return c(e.name_ncol as usize);
    }
    let m = &e.marks;
    let monster = e.flags & 1 != 0;
    let pc = id >> 24 == 0;
    if e.kind == EntityKind::Npc && matches!(e.sub_kind, 2..=4) {
        return c(4);
    }
    if matches!(e.motion.status, 2 | 3) {
        return c(9);
    }
    if m.gm >= 3 {
        return c(0x0c + m.gm as usize - 3);
    }
    // Claimed: by us or our party red, another party of the alliance a red / magenta blend, others magenta.
    if monster && e.claim != 0 {
        let own = w.party.own_flags & 3;
        if e.claim == w.my_id {
            return c(6);
        }
        return match w.party.members().find(|r| r.id == e.claim) {
            Some(r) if r.flags & 3 == own => c(6),
            Some(_) => (c(6) >> 1) + (c(7) >> 1),
            None => c(7),
        };
    }
    if id != w.my_id && w.party.party().any(|r| r.id == id) {
        return c(1);
    }
    if m.yell {
        return c(8);
    }
    if m.lfg_master || m.auto || m.lfg {
        return c(2);
    }
    if m.anon {
        return c(3);
    }
    if pc || (m.alleg == 1 && !m.charm) {
        return c(0);
    }
    if !m.charm {
        return c(if monster { 5 } else { 4 });
    }
    if m.npc_pet || m.b2b1 {
        return c(if m.trust && e.index == w.my_pet { 1 } else { 4 });
    }
    c(1)
}

/// FUN_100818c0 / FUN_100819d0 at view depth `d`: retail's z_ndc * 4096 (near 0.1, far 65535) is 1 up to
/// 4020 and (4100 - z) / 80 beyond, so plates shrink as about 0.05 + 5.12 / d past d = 5.39.
fn plate_scale(d: f32) -> Option<f32> {
    const NEAR: f64 = 0.1;
    const FAR: f64 = 65535.0;
    if d < 1.0 {
        return None;
    }
    let z = (4096.0 * FAR / (FAR - NEAR) * (1.0 - NEAR / d as f64)) as i32;
    match z {
        _ if z > 4100 => None,
        _ if z < 4020 => Some(1.0),
        _ => Some((4100 - z) as f32 / 80.0),
    }
}

/// Music and effects volume: configs 10 / 9, times the /mutebgm and /mutese factors.
fn world_volumes(w: &World) -> (f32, f32) {
    let f = |id: u16, mute: bool| if mute { 0.0 } else { w.game.get(id) as f32 / 100.0 };
    (f(10, w.bgm_mute), f(9, w.se_mute))
}

/// FUN_100973f0, InvisFlag on a PC: our own model at colour 0x27808080, another's at alpha 0, and no plate. Not retail:
/// a party or alliance member stays at 0x27 with its plate (an improvement, the user's call 2026-10-25).
fn invisible_alpha(w: &World, id: u32) -> Option<f32> {
    let translucent = 0x27 as f32 / 128.0;
    if id == w.my_id {
        return w.my_plate.invisible.then_some(translucent);
    }
    if !w.entities.get(&id).is_some_and(|e| id >> 24 == 0 && e.invisible) {
        return None;
    }
    Some(if w.party.members().any(|r| r.id == id) { translucent } else { 0.0 })
}

/// The name plate fields of an entity (client/plate.rs).
fn entity_plate(id: u32, e: &Entity) -> plate::Plate {
    plate::Plate { pc: id >> 24 == 0, npc: id >> 24 == 1, marks: e.marks, extra: e.plate, monstrosity: e.monstrosity, sync: e.sync, mon_stat: e.sub[0] }
}

/// Ours, from 0x037 and our 0x067.
fn self_plate(world: &World) -> plate::Plate {
    let p = &world.my_plate;
    plate::Plate { pc: true, npc: false, marks: p.marks, extra: p.extra, monstrosity: p.monstrosity, sync: world.my_sync, mon_stat: 0 }
}

fn name_plates(gu: &mut GameUi, world: &World, sw: f32, sh: f32, view_proj: glam::Mat4) {
    let project = |p: Vec3| -> Option<(f32, f32, [f32; 2], f32)> {
        let clip = view_proj * p.extend(1.0);
        let s = plate_scale(clip.w)?;
        let ndc = clip.xyz() / clip.w;
        // FUN_10084b20: viewport mapping, then -0.5 (FUN_10026220); anchors up to 100 px off screen still draw.
        let (x, y) = ((ndc.x + 1.0) / 2.0 * sw - 0.5, (1.0 - ndc.y) / 2.0 * sh - 0.5);
        (x.abs() <= sw + 100.0 && y.abs() <= sh + 100.0).then_some((x, y, [sw * 0.00234375 * s, sh * 0.00234375 * s], ndc.z))
    };
    let hidden = event::hidden(world);
    // Retail draws plates into the scene target before the op 0x0f multiply (FUN_10010d20, MODLOG "Name plates under
    // the screen fade"); ours scale their colour by it, which is the same for an alpha-blended plate.
    let m = world.screen.multiply().map(|c| c * (1.0 - world.fade));
    let faded = |tint: u32| -> u32 {
        let ch = |sh: u32, k: usize| (((tint >> sh & 0xff) as f32 * m[k]).round().min(255.0) as u32) << sh;
        tint & 0xff00_0000 | ch(16, 0) | ch(8, 1) | ch(0, 2)
    };
    // Invisible helpers (no model or a stub one) only show a name while targeted; faded actors none.
    let visible = |id: &u32, e: &Entity| {
        let faded = world.actors.get(id).is_some_and(|a| a.vanished());
        !faded && (e.kind == EntityKind::Player || world.actors.get(id).is_some_and(|a| a.shown()) || world.target == Some(*id))
    };
    // FUN_100d0510: /names off hides every name, /names otherpc other players' only.
    let named = |id: &u32| match world.names_hidden {
        None => true,
        Some(others) => others && (*id >> 24 != 0 || *id == world.my_id),
    };
    for (id, e) in world.entities.iter().filter(|(id, e)| !e.name.is_empty() && !e.name.starts_with('_') && !e.no_plate && visible(id, e) && !hidden.contains(id) && named(id) && invisible_alpha(world, **id).is_none_or(|k| k > 0.0)) {
        if let Some((x, y, scale, z)) = plate_point(world, *id).and_then(project) {
            // FUN_100819d0: the target's plate alpha pulses, a * (sin(ticks * 16 deg) * 32 + 96) / 128.
            let pulse = if world.target == Some(*id) {
                let t = world.started.elapsed().as_secs_f32() * 60.0;
                (((t * 16.0) % 360.0).to_radians().sin() * 32.0 + 96.0) / 128.0
            } else {
                1.0
            };
            // Pre-transformed quads at the projected depth (FUN_10080100): walls in front hide them.
            gu.depth(Some(z));
            let tint = name_colour(*id, e, world, |i| gu.ncol(i));
            let p = entity_plate(*id, e);
            let (prefix, icon) = plate::prefix(&p);
            gu.name_plate(&prefix, &e.name, x, y, scale, faded(tint), pulse, faded(icon), plate::stars(&p));
        }
    }
    if !hidden.contains(&world.my_id)
        && named(&world.my_id)
        && !world.my_plate.invisible
        && let Some((x, y, scale, z)) = plate_point(world, world.my_id).and_then(project)
    {
        gu.depth(Some(z));
        // FUN_1008db40 on our own actor: the GM colours from 0x037's GM level.
        let gm = world.my_plate.marks.gm as usize;
        let tint = faded(gu.ncol(if gm >= 3 { 0x0c + gm - 3 } else { 0 }));
        let p = self_plate(world);
        let (prefix, icon) = plate::prefix(&p);
        gu.name_plate(&prefix, &world.name, x, y, scale, tint, 1.0, faded(icon), plate::stars(&p));
    }
    gu.depth(None);
}

/// Where the target cursor points (FUN_10153470 -> vtable +0x27c, FUN_100ce6d0): attach point 2 like the name
/// plate (+0x8a6 = +0x8a7 = 2), projected and truncated; the plate hangs below the same point.
fn target_point(world: &World, id: u32, sw: f32, sh: f32, view_proj: glam::Mat4) -> Option<(f32, f32)> {
    let p = plate_point(world, id)?;
    let clip = view_proj * p.extend(1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let ndc = clip.xyz() / clip.w;
    Some((((ndc.x + 1.0) / 2.0 * sw).trunc(), ((1.0 - ndc.y) / 2.0 * sh).trunc()))
}

/// FUN_100d23f0 for attach point 2 (FUN_1002b5e0): the actor position plus the skeleton's point 2 offset times
/// the model scale, rest data only, so it does not move with the pose; then FUN_1002b7a0's lift by status
/// for bone-0 points (5 chocobo -1.3, 85 mount by its variant, 63..83 chairs -0.5). Without points: the mesh top.
fn plate_point(world: &World, id: u32) -> Option<Vec3> {
    // A door's screen point (XiDoorActor vtable +0x27c, FUN_100ac390) is its own position (+0x1bc), unlifted.
    if let Some(e) = world.entities.get(&id).filter(|e| e.door != 0) {
        return Some(e.pos);
    }
    let (pos, status) = if id == world.my_id { (drawn_pos(world), world.my_motion.status) } else { world.entities.get(&id).map(|e| (e.pos, e.motion.status))? };
    let pos = event::pose(world, id).map_or(pos, |p| p.pos);
    // The drawn position (+0x678) has the mount offset; `coff` keeps the riding status.
    let (pos, status) = (pos + ride::offset(world, id), ride::applied(world, id, status));
    let a = world.actors.get(&id).filter(|a| a.gpu.is_some());
    let point = a.and_then(|a| a.character.as_ref()?.skeleton.points.get(2).copied().map(|p| (p, a.scale)));
    match point {
        Some(((bone, t), s)) => {
            let lift = match status {
                5 if bone == 0 => -1.3,
                85 if bone == 0 => ride::plate_lift(world, id),
                63..=83 if bone == 0 => -0.5,
                _ => 0.0,
            };
            Some(pos + t * s + Vec3::new(0.0, lift * s, 0.0))
        }
        None => Some(pos + Vec3::new(0.0, a.map_or(-2.0, |a| a.top) - 0.1, 0.0)),
    }
}

/// The in-game HUD in the game's own windows: chat log and input line, party, target, main menu
/// ('-' opens it) and the KO prompt. Returns the rects that take clicks away from the world.
fn world_hud(gu: &mut GameUi, world: &mut World, input: &UiInput, size: [u32; 2], keys: &mut HashSet<KeyCode>, view_proj: glam::Mat4, numpad_slash: bool) -> Vec<[f32; 4]> {
    let (sw, sh) = (size[0] as f32, size[1] as f32);
    // The focused list's scroll bar and the wheel, before the windows see the press.
    let taken = scrollbar::frame(gu, world, input);
    let input = taken.as_ref().unwrap_or(input);
    // FUN_1022eca0: keytops3 165 + (n >> 2) % 5 at (W - 234, H - 48 + 8 - min(2n, 8)), y scale min(n / 4, 1), n
    // the draws since the open (taken at 30 a second, inferred); the HUD windows are not drawn meanwhile.
    if let Some(t) = world.downloading {
        gu.clear();
        downloading(gu, (t.elapsed().as_secs_f32() * 30.0) as usize + 1, sw, sh);
        return Vec::new();
    }
    // Scroll Lock hid the UI: nothing draws; Esc shows it again (FUN_101202a0 consumes the key).
    if world.ui_hidden {
        gu.clear();
        if input.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::Escape, pressed: true, .. })) {
            world.ui_hidden = false;
        }
        return Vec::new();
    }
    for mut c in std::mem::take(&mut world.map_cmds) {
        if let map::Cmd::Preview { point_y, .. } = &mut c {
            *point_y = preview_point_y(gu, world, sw, sh);
        }
        map::apply(world, gu.map_regions(), c, sw, sh);
    }
    if let Some(mode) = world.region_cmd.take() {
        region::command(world, gu, mode, sw, sh);
    }
    map::refresh(world, gu);
    items::check_iuse(world);
    helpdesk::keep_scroll(world, gu, sw, sh);
    let items = gu.menu_items(sw, sh, world.menu_page);
    let dead = world.dead();
    let mut opened = false;
    let mut shrink_log = false;
    let mut pick: Option<usize> = None;
    let mut answer: Option<bool> = None;
    let mut act: Option<u8> = None;
    // The event input window and the map's marker name entry take every key first.
    let map_typing = world.map.as_ref().is_some_and(|m| m.typing());
    let post_typing = post::typing(world);
    let search_typing = searchmenu::typing(world);
    let ls_typing = linkshell::typing(world);
    let comment_typing = seacom::typing(world);
    let macro_typing = macroedit::typing(world);
    let faq_typing = faqmain::typing(world);
    let input_open = event::input(world).is_some() || map_typing || post_typing || search_typing || ls_typing || comment_typing || macro_typing || faq_typing;
    if post_typing {
        post::text_events(world, &input.events);
        keys.clear();
    } else if search_typing {
        searchmenu::text_events(world, &input.events);
        keys.clear();
    } else if ls_typing {
        linkshell::text_events(world, &input.events);
        keys.clear();
    } else if comment_typing {
        seacom::text_events(world, &input.events);
        keys.clear();
    } else if macro_typing {
        macroedit::text_events(world, &input.events);
        keys.clear();
    } else if faq_typing {
        faqmain::text_events(world, &input.events);
        keys.clear();
    } else if map_typing && let Some(m) = world.map.as_mut() {
        m.text_events(&input.events);
        keys.clear();
    } else if input_open {
        event::input_events(world, &input.events);
        keys.clear();
    }
    for ev in input.events.iter().filter(|_| !input_open) {
        match ev {
            egui::Event::Text(t) | egui::Event::Paste(t) if world.typing && !opened => {
                world.line_hist.edited();
                // Space moves the open candidate list instead (FUN_101b6810 returns -1).
                let open = world.fep.is_some();
                let t: String = t.chars().filter(|c| !c.is_control() && !(open && *c == ' ')).collect();
                fep::insert(world, &t);
            }
            // egui turns Ctrl+C / Ctrl+X into Copy / Cut instead of key events. In the chat line they copy / cut the
            // selection (ours; Ctrl+V pastes through egui-winit's clipboard), else /check and the /linkshell2 line.
            egui::Event::Copy if world.typing => world.copied = fep::selected_text(world),
            egui::Event::Copy if !event::active(world) => check_target(world),
            egui::Event::Cut if world.typing && fep::selection(world).is_some() => {
                world.copied = fep::selected_text(world);
                fep::delete_selection(world);
            }
            egui::Event::Cut => chat_prefix(world, "/linkshell2 "),
            egui::Event::Key { key, pressed: true, modifiers, .. } => match key {
                // The auto-translate list takes its keys first (FUN_101b5d10).
                k if world.typing && world.fep.is_some() && fep::key(world, *k, modifiers.shift) => {}
                egui::Key::Tab if world.typing => fep::open(world),
                // FUN_101ac2c0 / FUN_101ac3d0: the line history (no sound, no wrap; Shift / Ctrl the same).
                egui::Key::ArrowUp if world.typing && !modifiers.alt => {
                    if let Some(l) = world.line_hist.up(&world.chat_input) {
                        (world.chat_input, world.chat_caret, world.chat_anchor) = (l, None, None);
                    }
                }
                egui::Key::ArrowDown if world.typing && !modifiers.alt => {
                    if let Some(l) = world.line_hist.down(&world.chat_input) {
                        (world.chat_input, world.chat_caret, world.chat_anchor) = (l, None, None);
                    }
                }
                egui::Key::Enter if world.typing => {
                    world.typing = false;
                    world.sounds.push(crate::audio::Ui::Select as u32);
                    let text = std::mem::take(&mut world.chat_input);
                    (world.chat_caret, world.chat_anchor) = (None, None);
                    world.line_hist.record(&text);
                    let text = fep::outgoing(world, text);
                    textcmd::run(world, &text, textcmd::Mode::Typed);
                }
                egui::Key::Escape if world.typing => {
                    world.typing = false;
                    (world.chat_input, world.chat_caret, world.chat_anchor) = (String::new(), None, None);
                    world.sounds.push(crate::audio::Ui::Cancel as u32);
                }
                egui::Key::Backspace | egui::Key::Delete if world.typing => {
                    world.line_hist.edited();
                    fep::erase(world, *key == egui::Key::Backspace);
                }
                egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::Home | egui::Key::End if world.typing => fep::caret_key(world, *key, modifiers.shift),
                egui::Key::A if world.typing && modifiers.ctrl => fep::select_all(world),
                // Ctrl / Alt shortcuts (DAT_103619f0 flag 0x10, FUN_101a5e80). Mode 2 (chat) works while typing.
                k if (modifiers.ctrl || modifiers.alt) && !opened && let Some(p) = chat_shortcut(*k) => {
                    chat_prefix(world, p);
                    keys.clear();
                }
                egui::Key::R if modifiers.ctrl || modifiers.alt => {
                    reply(world);
                    keys.clear();
                }
                _ if world.typing => {}
                // An event takes the keys: Enter advances or picks, arrows move, Escape cancels.
                _ if event::active(world) => event::key(world, *key),
                egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::Enter | egui::Key::Escape if world.full_log.is_some() => {
                    full_log_key(world, *key, modifiers.ctrl || modifiers.alt, sh);
                }
                // cnqframe takes the arrows (its cursor) and Escape (FUN_101e8b10).
                egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::Enter | egui::Key::Escape if world.cnq.is_some() => {
                    region::key(world, *key);
                }
                egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::Enter | egui::Key::Escape if world.map.is_some() => {
                    // map0's Escape (FUN_101fa420): the zoom into config 0x8a, then the fade-out.
                    if let Some(m) = world.map.as_mut()
                        && !m.key(*key)
                    {
                        world.game.set(0x8a, m.zoom_cfg());
                        m.begin_close();
                    }
                }
                egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::Enter | egui::Key::Escape if world.log_focus == 2 => {
                    buffs::key(gu, world, *key);
                }
                // logwindo's messages (FUN_1015ae20): 5 Enter opens the full log, 6 Escape unfocuses.
                egui::Key::Enter if world.log_active => {
                    world.full_log = Some(logwin::FullLog::open(sh));
                    world.log_active = false;
                }
                // The map overlay closes first (after the windows, below).
                egui::Key::Escape if world.log_active && world.overlay.is_none() => world.log_active = false,
                egui::Key::A if modifiers.ctrl || modifiers.alt => send_chat(world, "/attack".into()),
                // Extra: the map overlay (a free Ctrl / Alt letter in retail's key set).
                egui::Key::O if (modifiers.ctrl || modifiers.alt) && world.map_overlay => {
                    if world.overlay.take().is_none() {
                        let o = map::Overlay::open(world, gu.map_regions());
                        if !o.has_map() {
                            world.sounds.push(crate::audio::Ui::Error as u32);
                        }
                        world.overlay = Some(o);
                    }
                }
                // The palette (FUN_101a3960): Left / Right move the cursor (wrapping), Down / Up the next / previous set,
                // Enter runs the cell and closes, Escape closes.
                _ if world.palette.0 != 0 => {
                    let p = world.palette.0 as usize - 1;
                    match key {
                        egui::Key::ArrowLeft | egui::Key::ArrowRight => {
                            let step = if *key == egui::Key::ArrowLeft { 9 } else { 1 };
                            world.palette.3[p] = (world.palette.3[p] + step) % 10;
                            world.sounds.push(crate::audio::Ui::Cursor as u32);
                        }
                        egui::Key::ArrowDown | egui::Key::ArrowUp => {
                            if let Some(m) = world.macros.as_mut() {
                                let (book, set) = (m.book, m.set);
                                m.switch(book, if *key == egui::Key::ArrowDown { (set + 1) % 10 } else { (set + 9) % 10 });
                            }
                        }
                        egui::Key::Enter => {
                            world.sounds.push(crate::audio::Ui::Select as u32);
                            let n = world.palette.3[p] + 10 * p;
                            if let Some(m) = world.macros.as_mut() {
                                m.start(n);
                            }
                            world.palette = (0, 0, 0, world.palette.3);
                        }
                        egui::Key::Escape => {
                            world.palette = (0, 0, 0, world.palette.3);
                            world.sounds.push(crate::audio::Ui::Cancel as u32);
                        }
                        _ => {}
                    }
                }
                // The target prompt (key table 0x10371ee0 with +0xc4 set): Enter confirms, Escape cancels.
                // The party-list cursor takes the window keys (its window is raised to the top).
                _ if world.party_cursor.is_some() => {
                    target::party_cursor_key(world, *key);
                }
                _ if world.prompt.is_some() && matches!(key, egui::Key::Enter | egui::Key::Escape) => {
                    if *key == egui::Key::Enter {
                        target::confirm_sub(world);
                    } else {
                        target::cancel_sub(world);
                        world.sounds.push(crate::audio::Ui::Cancel as u32);
                    }
                }
                // Ctrl / Alt + M, J, W, B (key functions 0x41, 0x7f, 0x80, 0x9f): magic in list mode without trusts, job
                // abilities, weapon skills, pet commands; in the field, alive, no prompt.
                egui::Key::M | egui::Key::J | egui::Key::W | egui::Key::B if (modifiers.ctrl || modifiers.alt) && !event::active(world) && !world.dead() && !target::sub_up(world) => match key {
                    egui::Key::M => magic::open_magic(world, 0, true, 0x100),
                    egui::Key::J => magic::open_ability(world, 1, false),
                    egui::Key::W => magic::open_ability(world, 3, false),
                    _ => magic::open_ability(world, 2, false),
                },
                _ if world.bluset.is_some() && world.confirm.is_none() => bluset::key(world, *key),
                _ if world.prompt.is_none() && world.confirm.is_none() && (world.cats.is_some() || world.magic.is_some() || world.ability.is_some()) => {
                    magic::key(world, *key);
                }
                egui::Key::H if modifiers.ctrl || modifiers.alt => {
                    world.sounds.push(crate::audio::Ui::Select as u32);
                    send_chat(world, "/heal".into());
                }
                egui::Key::C if modifiers.ctrl || modifiers.alt => check_target(world),
                // FUN_1012ec60 / FUN_101e9f10.
                egui::Key::E if modifiers.ctrl || modifiers.alt => items::open_equipment(world),
                egui::Key::I if modifiers.ctrl || modifiers.alt => items::open_items(world),
                // Our settings screen, then the Config windows, take the keys while open.
                _ if world.settings.is_some() => {
                    let mut s = world.settings.take().unwrap();
                    if !s.key_mod(*key, modifiers.ctrl || modifiers.alt) {
                        world.settings = Some(s);
                    } else {
                        world.sounds.push(crate::audio::Ui::Cancel as u32);
                    }
                    if let Some(s) = &world.settings {
                        world.sounds.extend(s.sounds.iter().map(|x| *x as u32));
                    }
                }
                _ if world.config.is_some() => {
                    let out = world.config.as_mut().unwrap().key(gu, &mut world.game, *key, sw);
                    config_out(world, out);
                }
                // The yes / no prompt (comyn) takes the keys while it is up, over any window: only Left / Right move (its
                // links), Enter answers, Escape is No.
                _ if world.confirm.is_some() => match key {
                    egui::Key::ArrowLeft | egui::Key::ArrowRight => {
                        if let Some((_, yes)) = &mut world.confirm {
                            *yes = !*yes;
                        }
                        world.sounds.push(crate::audio::Ui::Cursor as u32);
                    }
                    egui::Key::Enter => {
                        answer = world.confirm.map(|c| c.1);
                        world.sounds.push(crate::audio::Ui::Select as u32);
                    }
                    egui::Key::Escape if matches!(world.confirm, Some((Confirm::Merit(_) | Confirm::JobPoint(_), _))) => {
                        world.sounds.push(crate::audio::Ui::Error as u32);
                    }
                    egui::Key::Escape => {
                        answer = Some(false);
                        world.sounds.push(crate::audio::Ui::Cancel as u32);
                    }
                    _ => {}
                },
                _ if world.inspect.up() => inspect::key(world, gu, *key),
                _ if world.win.is_some() => {
                    items::key(world, *key, modifiers.ctrl || modifiers.alt, modifiers.alt);
                }
                _ if shop::active(world) => shop::key(world, *key),
                _ if trade::active(world) => trade::key(world, gu, *key),
                _ if synth::active(world) => synth::key(world, gu, *key),
                _ if post::active(world) => post::key(world, gu, *key),
                // The pool windows over the action menu (loot, lootope, lnowin).
                _ if treasure::active(world) => treasure::key(world, *key),
                _ if world.chatctrl.is_some() => chatctrl::key(world, *key),
                _ if world.arealist.is_some() => arealist::key(world, gu, *key),
                _ if world.levelsync.is_some() => levelsync::key(world, *key),
                _ if searchmenu::refining(world) => searchmenu::key(world, gu, *key),
                _ if world.search.is_some() => search::key(world, *key),
                _ if world.searchmenu.is_some() => searchmenu::key(world, gu, *key),
                _ if world.partymenu.is_some() => partymenu::key(world, gu, *key),
                _ if world.viewhouse.is_some() => viewhouse::key(world, gu, *key),
                _ if world.bazaar.is_some() => bazaar::key(world, *key),
                _ if world.currencies.is_some() => currencies::key(world, *key),
                _ if world.unity.is_some() => unity::key(world, gu, *key, modifiers.ctrl || modifiers.alt),
                _ if world.pupequip.is_some() => equipwin::pup_key(world, gu, *key),
                _ if world.merits_ui.is_some() => merits::key(world, gu, *key),
                _ if world.jp_ui.is_some() => jobpoints::key(world, *key, modifiers.ctrl || modifiers.alt),
                _ if world.ae_ui.is_some() => aepoints::key(world, *key, modifiers.ctrl || modifiers.alt),
                _ if world.status.is_some() => status::key(world, gu, *key),
                _ if world.missions.is_some() => missions::key(world, gu, *key, modifiers.ctrl || modifiers.alt),
                _ if world.quests.is_some() => quests::key(world, *key, modifiers.ctrl || modifiers.alt),
                _ if world.keyitems.is_some() => keyitems::key(world, gu, *key, modifiers.ctrl || modifiers.alt),
                _ if helpdesk::focused(world) => helpdesk::key(world, *key),
                _ if world.macroedit.is_some() => macroedit::key(world, gu, *key),
                _ if world.blklist.is_some() => blklist::key(world, *key),
                _ if world.linklist.is_some() => linklist::key(world, gu, *key),
                _ if world.linkshell.is_some() => linkshell::key(world, gu, *key),
                _ if world.region_menu.is_some() => region::menu_key(world, gu, *key, sw, sh),
                _ if world.moogle.is_some() => {
                    moogle::key(world, gu, *key);
                }
                // FUN_101d6540: the Mog House door menu.
                _ if world.mogdoor.is_some() => {
                    let m = world.mogdoor.as_mut().unwrap();
                    let n = m.rows().max(1);
                    match key {
                        egui::Key::ArrowDown | egui::Key::ArrowUp => {
                            m.sel = if *key == egui::Key::ArrowDown { (m.sel + 1) % n } else { (m.sel + n - 1) % n };
                            world.sounds.push(crate::audio::Ui::Cursor as u32);
                        }
                        egui::Key::Enter => mogdoor_pick(world),
                        egui::Key::Escape => {
                            if m.back() {
                                world.mogdoor = None;
                            }
                            world.sounds.push(crate::audio::Ui::Cancel as u32);
                        }
                        _ => {}
                    }
                }
                // The action menu (FUN_1012bd10) takes the keys while it is up (hidden under a sub-target prompt).
                // Escape engaged keeps it: autorun stops, else the log takes a line off; out of battle it closes.
                // Right on Disengage / Call for help swaps them (FUN_1012acc0), on Magic opens magselec.
                // Only its own keys: Space (the chat line) and the rest go on as usual.
                egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::Enter | egui::Key::Escape
                    if world.action_menu.is_some() && world.prompt.is_none() =>
                {
                    let engaged = world.my_motion.status == 1;
                    let m = world.action_menu.as_mut().unwrap();
                    let n = m.items.len().max(1);
                    let row = m.items.get(m.sel).copied();
                    match key {
                        egui::Key::ArrowDown | egui::Key::ArrowUp => {
                            m.sel = if *key == egui::Key::ArrowDown { (m.sel + 1) % n } else { (m.sel + n - 1) % n };
                            world.sounds.push(crate::audio::Ui::Cursor as u32);
                        }
                        egui::Key::ArrowRight => {
                            combat::right(world);
                        }
                        egui::Key::Enter => act = row,
                        egui::Key::Escape if engaged => {
                            if world.autorun {
                                world.autorun = false;
                            } else {
                                shrink_log = true;
                            }
                        }
                        egui::Key::Escape => {
                            world.action_menu = None;
                            world.sounds.push(crate::audio::Ui::Cancel as u32);
                        }
                        _ => {}
                    }
                }
                // The menu opens on the page it was closed on.
                // While dead the K.O. window or the raise prompt has the focus and takes it (message 7, FUN_1015dd00).
                egui::Key::Minus if !world.ko_open && world.ko_offer != 2 => world.menu = if world.menu.is_some() { None } else { Some(world.menu_mem[world.menu_page as usize & 1]) },
                egui::Key::Escape if world.menu.is_some() => world.menu = None,
                egui::Key::Escape if world.overlay.is_some() => world.overlay = None,
                // FUN_102110c0: the raise prompt refuses Escape (+0x39 = 0: SE 0004) and keeps the key.
                egui::Key::Escape if dead && world.ko_offer == 2 && world.confirm.is_none() => world.sounds.push(crate::audio::Ui::Error as u32),
                // Escape also takes a line off the log (key 0x95, FUN_1015b830). Clearing the target is key 0x92,
                // refused while locked (FUN_10151f20).
                egui::Key::Escape => {
                    if target::picks_allowed(world) {
                        target::set(world, None);
                    }
                    shrink_log = true;
                }
                egui::Key::ArrowDown if world.menu.is_some() => world.menu = world.menu.map(|m| (m + 1) % items.len().max(1)),
                egui::Key::ArrowUp if world.menu.is_some() => world.menu = world.menu.map(|m| (m + items.len().max(1) - 1) % items.len().max(1)),
                // Left / right swap the commands and social pages on the same row (FUN_1013b3c0).
                egui::Key::ArrowLeft | egui::Key::ArrowRight if world.menu.is_some() => world.menu_page ^= 1,
                egui::Key::Enter if world.menu.is_some() => pick = world.menu,
                // The raise prompt (resynus, FUN_102110c0): Up to Accept, Down to Decline (no wrap), Enter answers.
                _ if dead && world.ko_offer == 2 && world.confirm.is_none() => match key {
                    egui::Key::ArrowUp | egui::Key::ArrowDown => {
                        let yes = *key == egui::Key::ArrowUp;
                        if yes != world.raise_yes {
                            world.raise_yes = yes;
                            world.sounds.push(crate::audio::Ui::Cursor as u32);
                        }
                    }
                    egui::Key::Enter => {
                        let yes = world.raise_yes;
                        answer_raise(world, yes);
                        world.sounds.push(crate::audio::Ui::Select as u32);
                    }
                    _ => {}
                },
                // The K.O. window (FUN_1012e030 message 5): Enter asks "Return to Home Point?" (comyn, default No).
                egui::Key::Enter if dead && world.ko_open => {
                    world.confirm = Some((Confirm::HomePoint, false));
                    world.sounds.push(crate::audio::Ui::Select as u32);
                }
                // Enter on an NPC target talks to it (FUN_101524e0); otherwise it opens the action menu
                // (FUN_10127790). Inferred: monsters (0x00E @0x25 bit 0x08) take the menu path.
                // With no target it first targets the nearest actor in front (FUN_101587f0 case 0); only with none
                // does the menu ("menu playermo") open.
                egui::Key::Enter if !dead => {
                    if !(world.target.is_none() && !event::active(world) && target::enter_nearest(world)) {
                        target::confirm_target(world);
                    }
                }
                egui::Key::Space => {
                    world.typing = true;
                    opened = true;
                    keys.clear();
                }
                // FUN_101aa930 -> FUN_101ac1a0: VK 0xBF opens the input; the '/' character that follows is typed.
                // egui reports numpad / as Slash too; that key is 0x45 / 0x88, not chat.
                egui::Key::Slash if !modifiers.ctrl && !modifiers.alt && !numpad_slash => {
                    world.typing = true;
                    keys.clear();
                }
                _ => {}
            },
            _ => {}
        }
    }

    // Mouse on the main menu: hover moves the cursor, a click picks; the arrows swap pages.
    if let (Some(sel), Some(p), None, None, None, None) = (world.menu, input.pointer, world.confirm, world.missions.as_ref(), world.quests.as_ref(), world.keyitems.as_ref())
        && world.linkshell.is_none()
    {
        if let Some(k) = items.iter().position(|(_, r)| inside(r, p)) {
            gu.hover.set(true);
            if input.moved || input.clicked {
                world.menu = Some(k);
            }
            if input.clicked && k == sel {
                pick = Some(k);
            }
        }
        if input.clicked && gu.menu_arrows(sw, sh, world.menu_page).iter().any(|r| inside(r, p)) {
            world.menu_page ^= 1;
        }
    }
    // Social page: child 11 Shut Down (FUN_10159f90), 12 Log Out (FUN_1015a020) ask first.
    world.menu_picked = false;
    if let Some(id) = pick.and_then(|k| items.get(k)).map(|(i, _)| i + 1).filter(|&id| !menu_disabled(world).contains(&(id as u16))) {
        world.menu_picked = true;
        // FUN_10140400: a row that opens its window plays 0002 (the rows below that can refuse push their own).
        if matches!((world.menu_page, id), (0, 1 | 2 | 3 | 4 | 5 | 6 | 8 | 10)) {
            world.sounds.push(crate::audio::Ui::Select as u32);
        }
        match (world.menu_page, id) {
            // Child 1 Status (FUN_10140400 case 5): persona, level and statcom2.
            (0, 1) => status::open(world),
            (0, 2) => items::menu_equipment(world),
            // Children 3 Magic (mgcmenu) and 5 Abilities (abimenu), FUN_10140400.
            (0, 3) => magic::open_cats(world, "menu    mgcmenu"),
            (0, 5) => magic::open_cats(world, "menu    abimenu"),
            (0, 4) => items::open_items(world),
            // Child 6 Party (FUN_10140400 case 6 -> FUN_102016e0).
            (0, 6) => partymenu::open(world),
            // Child 8 Search (FUN_10140400 case 8): searchma.
            (0, 8) => searchmenu::open(world),
            // Child 9 Linkshell (FUN_10140400 case 9): link5 and link6 beside the main menu, its own sounds.
            (0, 9) => linkshell::open(world),
            // Child 11 Region Info (FUN_10140400 case 0xb): "menu    region", SE 0002.
            (0, 11) => region::open_menu(world),
            // Child 7 Trade (FUN_10140400 case 7 -> FUN_1013fe70).
            (0, 7) => {
                if trade::start(world) {
                    world.sounds.push(crate::audio::Ui::Select as u32);
                }
            }
            // Child 10 Synthesis (FUN_10140400 case 10): cmbmenu.
            (0, 10) => synth::open_menu(world),
            // Item 12 in the Mog House (FUN_10140400 case 0xc): myroom, SE 0002, unless visiting another's (the row is
            // disabled then: help 0xe 0x2a).
            (0, 12) if world.myroom.is_some() => {
                if world.mh_owner == 0 {
                    world.sounds.push(crate::audio::Ui::Select as u32);
                    moogle::open(world, false);
                } else {
                    world.sounds.push(crate::audio::Ui::Error as u32);
                }
            }
            // Item 12 Map (FUN_1013b3f0 case 0xc).
            (0, 12) => match map::MapWin::open(world, gu.map_regions(), sw, sh) {
                Some(mut m) => {
                    world.sounds.push(crate::audio::Ui::Select as u32);
                    m.return_menu = world.menu;
                    world.map = Some(m);
                    world.menu = None;
                }
                // FUN_101f86b0: "There is no map for this area."
                None => textcmd::refuse(world, 0x62),
            },
            // Child 1 Missions (FUN_1014e7b0 case 5): missionm over the social menu, SE 0002.
            (1, 1) => {
                missions::open_menu(world);
                world.sounds.push(crate::audio::Ui::Select as u32);
            }
            // Child 2 Quests (FUN_1014e7b0 case 5): the /quest text command, SE 0002.
            (1, 2) => {
                textcmd::run(world, "/quest", textcmd::Mode::Internal);
                world.sounds.push(crate::audio::Ui::Select as u32);
            }
            // Child 3 Key Items (FUN_1014e7b0 case 5): the /keyitem text command, SE 0002.
            (1, 3) => {
                textcmd::run(world, "/keyitem", textcmd::Mode::Internal);
                world.sounds.push(crate::audio::Ui::Select as u32);
            }
            // Child 7 Config (FUN_10147fc0 case 7): configwi over the menu, SE 0002.
            (1, 7) => {
                world.config = Some(configui::ConfigUi::open("configwi"));
                world.menu = None;
                world.sounds.push(crate::audio::Ui::Select as u32);
            }
            // Child 9 Communication (FUN_1014e7b0): commenu, whose open hook asks for the emote unlocks (c2s 0x119).
            (1, 9) => {
                magic::open_cats(world, "menu    commenu");
                let _ = world.net.commands.send(NetCommand::Packet { id: 0x119, body: Vec::new() });
                world.sounds.push(crate::audio::Ui::Select as u32);
            }
            // Child 10 Current Time (FUN_10147fc0 case 10): the /clock text command, SE 0002 (FUN_10036d70).
            (1, 10) => {
                textcmd::run(world, "/clock", textcmd::Mode::Internal);
                world.sounds.push(crate::audio::Ui::Select as u32);
            }
            // Child 4 View House (FUN_1014e7b0 case 4): mogcont.
            (1, 4) => viewhouse::open(world),
            // Child 5 Set Bazaar: "menu    bazaar".
            (1, 5) => bazaar::open(world),
            // Child 6 MacroPalette: mcrmenu, SE 0002.
            (1, 6) => macroedit::open(world),
            // Child 8 Help Desk (FUN_102174c0): faqsub beside the social page, SE 0002.
            (1, 8) => {
                helpdesk::open(world);
                world.sounds.push(crate::audio::Ui::Select as u32);
            }
            (1, 11) => world.confirm = Some((Confirm::Shutdown, false)),
            (1, 12) => world.confirm = Some((Confirm::Logout, false)),
            _ => log::info!("main menu item {id} on page {} is not implemented yet", world.menu_page),
        }
        // The prompt leaves the menu open under it (FUN_1014e7b0 items 0xb / 0xc), unfocused.
        if world.win.is_some() {
            world.menu = None;
        }
    }
    // /mission waits for the UI (it matches the logs' titles in group 0xd).
    if let Some(arg) = world.mission_cmd.take() {
        missions::command(world, gu, arg.as_deref());
    }
    // Mouse on the settings screen, else the Config windows.
    if let (Some(s), Some(p)) = (world.settings.as_mut(), input.pointer) {
        s.pointer(gu, p, input.moved, input.clicked, sw, sh);
        if s.hover {
            gu.hover.set(true);
        }
        world.sounds.extend(s.sounds.drain(..).map(|x| x as u32));
    }
    if let (Some(c), Some(p), None) = (world.config.as_mut(), input.pointer, world.settings.as_ref()) {
        let out = c.pointer(gu, &mut world.game, p, input.moved, input.clicked, sw, sh);
        config_out(world, out);
    }
    // Mouse on an open window's item list.
    if world.win.is_some()
        && let Some(p) = input.pointer
    {
        items::pointer(world, gu, sw, sh, p, input.moved, input.clicked);
    }
    if let Some(p) = input.pointer.filter(|_| world.linkshell.is_some() && world.linklist.is_none() && world.confirm.is_none()) {
        linkshell::pointer(world, gu, sw, sh, p, input.moved, input.clicked);
    }
    if let Some(p) = input.pointer.filter(|_| world.arealist.is_some() && world.confirm.is_none()) {
        arealist::pointer(world, gu, sw, sh, p, input.moved, input.clicked);
    }
    if let Some(p) = input.pointer.filter(|_| world.blklist.is_some() && world.confirm.is_none()) {
        blklist::pointer(world, gu, sw, sh, p, input.moved, input.clicked, input.right_clicked);
    }
    if let Some(p) = input.pointer.filter(|_| world.loot.is_some() && world.confirm.is_none()) {
        treasure::pointer(world, gu, sw, sh, p, input.moved, input.clicked, input.right_clicked);
    }
    if let Some(p) = input.pointer.filter(|_| searchmenu::focused(world)) {
        searchmenu::pointer(world, gu, sw, sh, p, input.moved, input.clicked);
    }
    if let Some(p) = input.pointer.filter(|_| partymenu::focused(world)) {
        partymenu::pointer(world, gu, sw, sh, p, input.moved, input.clicked);
    }
    if let Some(p) = input.pointer.filter(|_| viewhouse::focused(world)) {
        viewhouse::pointer(world, gu, sw, sh, p, input.moved, input.clicked);
    }
    if let Some(p) = input.pointer.filter(|_| bazaar::focused(world)) {
        bazaar::pointer(world, gu, sw, sh, p, input.moved, input.clicked);
    }
    if let Some(p) = input.pointer.filter(|_| world.keyitems.is_some() && world.confirm.is_none()) {
        keyitems::pointer(world, gu, sw, sh, p, input.moved, input.clicked);
    }
    if let Some(p) = input.pointer.filter(|_| world.quests.is_some() && world.confirm.is_none()) {
        quests::pointer(world, gu, sw, sh, p, input.moved, input.clicked);
    }
    if let Some(p) = input.pointer.filter(|_| world.missions.is_some() && world.confirm.is_none()) {
        missions::pointer(world, gu, sw, sh, p, input.moved, input.clicked);
    }
    if let Some(p) = input.pointer.filter(|_| world.currencies.is_some() && world.confirm.is_none()) {
        currencies::pointer(world, gu, sw, sh, p, input.moved);
    }
    if let Some(p) = input.pointer.filter(|_| world.merits_ui.is_some() && world.confirm.is_none()) {
        merits::pointer(world, gu, sw, sh, p, input.moved, input.clicked, input.right_clicked);
    }
    if let Some(p) = input.pointer.filter(|_| world.status.is_some() && world.currencies.is_none() && world.unity.is_none() && world.merits_ui.is_none() && world.jp_ui.is_none() && world.confirm.is_none()) {
        status::pointer(world, gu, sw, sh, p, input.moved, input.clicked, input.right_clicked);
    }
    // Mouse on the magic / ability lists and their category windows, or the Set Spells screen over them.
    if let Some(p) = input.pointer.filter(|_| world.bluset.is_some() && world.confirm.is_none()) {
        bluset::pointer(world, gu, sw, sh, p, input.moved, input.clicked);
    } else if let Some(p) = input.pointer.filter(|_| world.cats.is_some() || world.magic.is_some() || world.ability.is_some()) {
        magic::pointer(world, gu, sw, sh, p, input.moved, input.clicked);
    }
    // The party cursor (<stpc>, a party menu pick): the mouse over the party windows' rows.
    if let Some(p) = input.pointer.filter(|_| world.party_cursor.is_some() && world.confirm.is_none()) {
        target::party_cursor_pointer(world, gu, sw, sh, p, input.moved, input.clicked, input.right_clicked);
    }
    // Mouse in an event: a click on a choice row picks it, anywhere else advances a prompt.
    if input.clicked && world.event.is_some() {
        let row = event::choice(world).and_then(|_| {
            let r = gu.window_rect("menu    query", sw, sh)?;
            let p = input.pointer?;
            gu.children("menu    query").iter().position(|c| inside(&[r[0] + c[0], r[1] + c[1], c[2].max(132.0), c[3].max(16.0)], p))
        });
        event::click(world, row);
    }

    // Mog House door menu rows (child n of mogdoor): hover moves the cursor, a click picks.
    let door_rows: Vec<[f32; 4]> = match (&world.mogdoor, gu.window_xy(myroom::WINDOW, sw, sh)) {
        (Some(m), Some((wx, wy))) => gu.children(myroom::WINDOW).iter().take(m.rows()).map(|c| [wx + c[0], wy + c[1], c[2].max(120.0), c[3].max(16.0)]).collect(),
        _ => Vec::new(),
    };
    if let (Some(p), Some(m)) = (input.pointer, world.mogdoor.as_mut())
        && let Some(k) = door_rows.iter().position(|r| inside(r, p))
    {
        gu.hover.set(true);
        let k = if m.list { k } else { myroom::ROWS.iter().position(|r| r.0 as usize == k + 1).unwrap_or(k) };
        if input.moved || input.clicked {
            m.sel = k;
        }
        if input.clicked {
            mogdoor_pick(world);
        }
    }

    // Action menu rows: hover moves the cursor; on the focused menu the left release over the cursor row picks it
    // (FUN_1015de50; on no row SE 0004), the button held 60 ticks on a row runs its Right function and starts again
    // (FUN_1012b4b0; a row change restarts it), and a right click is Escape (msg 6, FUN_1015def0; ours only over it).
    let action_rows = world.action_menu.as_ref().filter(|_| world.prompt.is_none()).and_then(|m| gu.window_rect(&combat::window_name(m.items.len()), sw, sh).map(|r| (r, combat::rows(r[0], r[1], m.items.len()))));
    let focused = world.magic.is_none() && world.ability.is_none() && world.cats.is_none() && world.win.is_none() && world.menu.is_none() && world.loot.is_none() && world.chatctrl.is_none();
    let (mut held, mut right_click) = (false, false);
    if let (Some((r, rows)), Some(p), Some(m)) = (&action_rows, input.pointer, world.action_menu.as_mut()) {
        match rows.iter().position(|r| inside(r, p)) {
            Some(k) => {
                gu.hover.set(true);
                if (input.moved || input.pressed) && k != m.sel {
                    m.sel = k;
                    m.hold = input.down.then(Instant::now);
                }
                if input.pressed {
                    m.hold = Some(Instant::now());
                } else if !input.down {
                    m.hold = None;
                }
                if focused && input.down && m.hold.is_some_and(|t| t.elapsed() >= std::time::Duration::from_secs(1)) {
                    held = true;
                    m.hold = Some(Instant::now());
                }
                if focused && input.released && k == m.sel {
                    act = m.items.get(k).copied();
                }
            }
            None => {
                m.hold = None;
                if focused && input.released && inside(r, p) {
                    world.sounds.push(crate::audio::Ui::Error as u32);
                }
            }
        }
        right_click = focused && input.right_clicked && inside(r, p);
    }
    if held {
        combat::right(world);
    }
    if right_click {
        if world.my_motion.status == 1 {
            world.autorun = false;
        } else {
            world.action_menu = None;
            world.sounds.push(crate::audio::Ui::Cancel as u32);
        }
    }
    if let Some(c) = act {
        // The generic window handler's Enter (FUN_10118260): 0002.
        world.sounds.push(crate::audio::Ui::Select as u32);
        if !combat::keeps_menu(c) {
            // The lists (Abilities, Magic, Items, Trust) only suspend the menu; it comes back with its cursor.
            let menu = world.action_menu.take();
            if matches!(c, 7 | 8 | 9 | 0x19) {
                world.held_menu = menu;
            }
        }
        if !combat::choose(world, c) {
            log::info!("action menu command {c} is not implemented yet");
        }
    }

    paginate(gu, world, logwin::text_width(sw, world.log.cfg.pct));
    // FUN_10123060 / FUN_10131e30: each page of the open message goes into the log as it opens.
    let page_lines: Vec<String> = match event::message(world) {
        Some(m) => match &m.pages {
            Some((_, pages)) if m.logged <= m.page.min(pages.len() - 1) => {
                let upto = m.page.min(pages.len() - 1);
                let lines = pages[m.logged..=upto].concat();
                m.logged = upto + 1;
                lines
            }
            _ => Vec::new(),
        },
        None => Vec::new(),
    };
    for l in page_lines {
        world.chat.push(l);
    }
    let me = (world.target == Some(world.my_id)).then(|| self_entity(world));
    let target = me.as_ref().or_else(|| world.target.and_then(|t| world.entities.get(&t)));
    // The party window's rows (slots 0..5): ours from our own vitals, the rest from the roster.
    let party_slots = world.party.slots();
    let rows: Vec<Option<party::Member>> = std::iter::once(None).chain(party_slots.slots[1..6].iter().take_while(|m| m.is_some()).cloned()).collect();
    let mine = world.vitals.map(|v| (v.3 as f32 / 100.0, v.4 as f32 / 100.0));
    // FUN_101cad00 per draw at retail's 30 fps: a += trunc((t - a) * 0.15), or a = t once that is 0. A row whose member
    // changed starts from 0 (its bars sweep up); a new target starts at its HP.
    let ids: Vec<(u32, u8, u8)> = rows.iter().map(|m| match m {
        None => (world.my_id, world.vitals.map_or(100, |v| v.3), world.vitals.map_or(100, |v| v.4)),
        Some(m) => (m.id, m.hpp, m.mpp),
    }).collect();
    world.party_gauges.resize(ids.len(), (0, 0, 0));
    for (g, id) in world.party_gauges.iter_mut().zip(&ids) {
        if g.0 != id.0 {
            *g = (id.0, 0, 0);
        }
    }
    let tgt = target.map(|t| (world.target.unwrap_or(0), t.hpp));
    if let Some((id, hpp)) = tgt
        && world.target_gauge.0 != id
    {
        world.target_gauge = (id, hpp);
    }
    let now = Instant::now();
    world.gauge_acc += world.gauge_last.map_or(0.0, |t| (now - t).as_secs_f32()).min(1.0);
    world.gauge_last = Some(now);
    let ease = |a: u8, t: u8| -> u8 {
        let d = ((t as f32 - a as f32) * 0.15) as i32;
        if d == 0 { t } else { (a as i32 + d) as u8 }
    };
    while world.gauge_acc >= 1.0 / 30.0 {
        world.gauge_acc -= 1.0 / 30.0;
        // FUN_10202530 eases only rows in our zone; an away row keeps its bars.
        for ((g, id), m) in world.party_gauges.iter_mut().zip(&ids).zip(&rows) {
            if m.as_ref().is_none_or(|m| m.away == 0 && m.zone == world.pos_zone) {
                (g.1, g.2) = (ease(g.1, id.1), ease(g.2, id.2));
            }
        }
        for (k, g) in world.ally_gauges.iter_mut().enumerate() {
            if let Some(m) = party_slots.slots.get(6 + k).and_then(|m| m.as_ref()) {
                g.1 = ease(g.1, m.hpp);
            }
        }
        if let Some((_, hpp)) = tgt {
            world.target_gauge.1 = ease(world.target_gauge.1, hpp);
        }
    }
    // raid1 / raid2 (FUN_10203c80): shown while their party exists and no window of the registry +0x2a 0x01 / 0x80
    // set is up (main and social menus, item / equipment windows, magic / ability lists, map, config, palettes) nor an
    // event holds us (the mapping to our state: inferred). Closing them drops a party-list cursor silently.
    let hide = world.menu.is_some() || world.win.is_some() || world.config.is_some() || world.map.is_some() || world.cnq.is_some() || world.magic.is_some() || world.ability.is_some() || world.palette.0 != 0 || event::active(world);
    world.ally_gauges.resize(12, (0, 0));
    let alliance: [Option<[Option<crate::render::screens::AllyRow>; 6]>; 2] = std::array::from_fn(|k| {
        (party_slots.counts[k + 1] > 0 && !hide).then(|| {
            std::array::from_fn(|i| {
                let s = 6 * (k + 1) + i;
                let m = party_slots.slots[s].as_ref()?;
                let g = &mut world.ally_gauges[s - 6];
                if g.0 != m.id {
                    *g = (m.id, m.hpp);
                }
                let leader = (m.id == party_slots.leaders[k + 1]) as u8 | ((m.id == party_slots.alliance_leader) as u8) << 1;
                Some(crate::render::screens::AllyRow { hp: m.hpp as f32 / 100.0, hp_anim: g.1 as f32 / 100.0, away: m.away != 0 || m.zone != world.pos_zone, marks: leader })
            })
        })
    });
    if hide && world.party_cursor.as_ref().is_some_and(|c| c.index >= 6) {
        world.party_cursor = None;
    }
    // FUN_1015fa00's group walk (GameUi::want; the gated windows say so themselves): suspended windows are not drawn,
    // their close plays, and their open when the bits are free again.
    let menu_off = world.action_menu.is_some() && !gu.want("menu    playermo");
    let target_off = target.is_some() && event::keeps_target(world) && !gu.want("menu    targetwi");
    let main_off = world.menu.is_some() && !gu.want(crate::render::screens::menu_window(world.menu_page));
    let st = crate::render::screens::HudState {
        party_anim: world.party_gauges.iter().map(|g| (g.1 as f32 / 100.0, g.2 as f32 / 100.0)).collect(),
        target_anim: tgt.map(|_| world.target_gauge.1 as f32 / 100.0),
        target_glyph: world.target.and_then(|id| if id == world.my_id { plate::target_glyph(&self_plate(world)) } else { world.entities.get(&id).and_then(|e| plate::target_glyph(&entity_plate(id, e))) }),
        party: rows.iter().filter_map(|m| match m {
            None => mine,
            Some(m) => Some((m.hpp as f32 / 100.0, m.mpp as f32 / 100.0)),
        }).collect(),
        party_window: if rows.len() > 1 || party_slots.leaders[0] != 0 { party_slots.window() } else { "menu    ptw0".into() },
        party_hand: target::party_hand(world).filter(|h| if h.0 < 6 { h.0 < rows.len() } else { alliance[h.0 / 6 - 1].is_some_and(|r| r[h.0 % 6].is_some()) }).map(|(row, tint, _)| (row, tint, gu.cursor_frame())),
        alliance,
        party_away: rows.iter().map(|m| m.as_ref().is_some_and(|m| m.away != 0 || m.zone != world.pos_zone)).collect(),
        party_marks: rows.iter().enumerate().map(|(k, m)| {
            let id = if k == 0 { world.my_id } else { m.as_ref().map_or(0, |m| m.id) };
            (id != 0 && id == party_slots.leaders[0]) as u8 | ((id != 0 && id == party_slots.alliance_leader) as u8) << 1
        }).collect(),
        target: target.filter(|_| event::keeps_target(world) && !target_off).map(|t| (!t.no_gauge).then_some(t.hpp as f32 / 100.0)),
        menu_selected: world.menu.filter(|_| !main_off).and_then(|k| items.get(k)).map(|(i, _)| *i),
        menu_page: world.menu_page,
        menu_off: menu_disabled(world),
        menu_swap: menu_swap(world),
        menu_focused: world.confirm.is_none() && world.missions.is_none() && world.quests.is_none() && world.keyitems.is_none() && world.linkshell.is_none() && world.helpdesk.is_none() && world.macroedit.is_none(),
        chat_log: false,
    };
    gu.clear();
    name_plates(gu, world, sw, sh, view_proj);
    if world.overlay.as_ref().is_some_and(|o| o.zone != world.pos_zone) {
        world.overlay = Some(map::Overlay::open(world, gu.map_regions()));
    }
    if let Some(o) = world.overlay.as_mut() {
        let facing = world.net.me.lock().map_or(0.0, |m| event::rad_of(m.dir));
        o.draw(gu, (world.pos.x, world.pos.z), facing, sw, sh, world.map_opacity);
    }
    if let Some(m) = world.cnq.as_mut() {
        region::poll_image(gu, m);
    }
    region::draw(gu, world, sw, sh);
    if let Some(m) = world.map.as_mut() {
        m.draw(gu, sw, sh);
        world.sounds.append(&mut m.sounds);
        let zone = m.zone();
        for out in std::mem::take(&mut m.outs) {
            let send = |w: &World, id: u16, v: u32| {
                let _ = w.net.commands.send(NetCommand::Packet { id, body: v.to_le_bytes().to_vec() });
            };
            match out {
                map::Out::AskParty => {
                    send(world, 0x0D2, zone as u32);
                    world.scan.party_busy = true;
                }
                // FUN_101fb7d0: the strip for either packet (0x0F6 while the last list is still open).
                map::Out::Scan => {
                    let id = world.scan.start();
                    send(world, id, (id == 0x0F4) as u32);
                    world.scan.busy = true;
                }
                map::Out::Track(i) => {
                    send(world, 0x0F5, i as u32);
                    world.scan.track(i);
                }
                map::Out::AskStop => world.confirm = Some((Confirm::StopTracking, false)),
                map::Out::NotBusy => world.scan.busy = false,
                // FUN_10099680: the masks are stale until s2c 0x063 type 6 answers.
                map::Out::AskPoints => {
                    let _ = world.net.commands.send(NetCommand::Packet { id: 0x114, body: Vec::new() });
                    world.point_masks = None;
                }
            }
        }
    }
    let slots = gu.hud(sw, sh, &st);

    // Target cursor (FUN_10149900): anc_l at the target's point, 6 frames of 4 ticks, tint 0xe0808080;
    // locked, windowps 212 over the target window (0x10149bc0).
    // In a prompt (FUN_101501a0 with +0xc4 set) the saved target keeps it and the pick gets a second one 10 px up,
    // tinted by range (FUN_10150610), and flashes (+0x88 |= 0x600) unless red.
    let frame = (world.target_since.elapsed().as_secs_f32() * 60.0 / 4.0) as usize % gu.lib_len("anc     anc_l").max(1);
    if let Some(p) = world.prompt.as_ref() {
        if let Some((x, y)) = p.saved.and_then(|t| target_point(world, t, sw, sh, view_proj)) {
            gu.sprite("anc     anc_l", frame, x.round(), y.round(), 0xe080_8080);
        }
        if let Some(t) = world.target {
            let tint = target::range_tint(world, t);
            if let Some((x, y)) = target_point(world, t, sw, sh, view_proj) {
                gu.sprite("anc     anc_l", frame, x.round(), (y - 10.0).round(), tint);
            }
        }
    }
    if let (Some(t), true, None) = (world.target, st.target.is_some(), world.prompt.as_ref()) {
        if let Some((x, y)) = target_point(world, t, sw, sh, view_proj) {
            gu.sprite("anc     anc_l", frame, x.round(), y.round(), 0xe080_8080);
        }
        if world.locked
            && let Some(r) = gu.window_rect("menu    targetwi", sw, sh)
        {
            gu.sprite("menu    windowps", 212, r[0], r[1], 0x8080_8080);
        }
    }
    // FUN_101626e0's keep: the chat line holds the log while it is at 2 lines or fewer (asm 0x1016280f), a window with
    // flag 0x1000 is up
    // (the choice menu *(inf)*), and while a message waits with no more rows showing than its page: older lines
    // decay away and the message holds.
    let page_rows = event::message_ref(world).and_then(|m| m.pages.as_ref().and_then(|(_, p)| p.get(m.page)).map(Vec::len)).unwrap_or(0);
    // The event input window sets the same input-line flag as the chat line (DAT_1062193c +0x4c).
    let line_open = world.typing || event::input(world).is_some();
    // FUN_101626e0's keep: FUN_1015ecc0(0x1000), any open window stacked on the log (the raise prompt, comyn, query,
    // shops, the lists ...), the chat line at 2 lines or fewer, a message page waiting.
    let stacked = gu.open_with_flag(0x1000) || (dead && world.ko_offer == 2) || event::choice(world).is_some() || world.confirm.is_some();
    let keep = (world.typing && world.log.at_most_two()) || stacked || (event::prompt(world) && world.log.lines() <= page_rows.max(1));
    // Another window taking the keys takes the focus; an event closes the full log (FUN_10133000).
    if world.menu.is_some() || world.win.is_some() || world.action_menu.is_some() || world.confirm.is_some() || world.config.is_some() || event::active(world) || (dead && world.ko_offer == 2) {
        world.log_active = false;
    }
    if event::active(world) && world.full_log.take().is_some() {
        world.log_focus = 0;
    }
    let stamps = logwin::Stamps { mode: world.game.get(0xb5), all: world.stamp_all_lines, colour: crate::dat::battle::argb_bytes(world.game.get(0x31)) };
    world.log.chat_line(world.typing);
    let tw = world.log.update(gu, &world.chat, sw, keep, shrink_log, world.log_active, line_open, stamps);
    let log = world.log.frame(sw, sh);
    gu.log_top = Some(log.rect[1]);
    gu.log_faded = world.log.faded();
    // FUN_101626e0 (asm 0x10162a52): with the log held by a stacked window, a height change moves the pointer with
    // it while it is inside one, so it stays on the same child. Ours shifts the drawn cursor (no warp on Wayland).
    if !stacked {
        world.ptr_dy = 0.0;
    } else if world.log.dh != 0
        && let Some(p) = input.pointer
        && gu.frames_with_flag(0x1000).iter().any(|r| p.0 >= r[0] && p.0 <= r[0] + r[2] && p.1 >= r[1] && p.1 <= r[1] + r[3])
    {
        world.ptr_dy -= world.log.dh as f32;
    }
    let event_window = event_window(gu, world, sw, sh, tw);
    let bars = bar_text(gu, world, &items);
    let cam = App::camera(world);
    let mut top = std::mem::take(&mut world.hud_top);
    // The compass follows region 0's top (UI object +0xa0, refreshed by FUN_10159080), i.e. the log's current top.
    top.draw(gu, world, sw, sh, &bars, cam.target - cam.eye, log.rect[1]);
    world.hud_top = top;
    let party = st.party_window.clone();
    let mut blockers: Vec<[f32; 4]> = [party.as_str()].iter().filter_map(|n| gu.window_rect(n, sw, sh)).collect();
    for (k, n) in ["menu    raid1", "menu    raid2"].iter().enumerate() {
        if st.alliance[k].is_some() {
            blockers.extend(gu.window_rect(n, sw, sh));
        }
    }
    blockers.extend(world.overlay.as_ref().filter(|o| o.has_map()).map(|o| o.rect));
    // Event text is in the log, drawn as usual; only placed narration (text codes 02 / 03) has its own window.
    if let Some(r) = event_window {
        blockers.push(r);
    } else if let Some(fl) = world.full_log {
        // The small logs are closed while the full log is open (FUN_1015c280).
        blockers.push(world.log.draw_full(gu, &fl, sw, sh, line_open, world.game.get(0xb4) != 0));
    } else if log.alpha > 0.0 || log.lines > 0 {
        let [x, y, w, h] = log.rect;
        gu.frame_alpha(x, y, w, h, log.alpha);
        let newest = world.log.draw(gu, &log, line_open);
        // A message waiting for a key ends in the kaipage arrow (FUN_1015ad10), one frame per draw.
        if event::prompt(world)
            && let Some((lx, ly, last)) = newest
        {
            let n = gu.lib_len("anc     kaipage").max(1);
            let frame = (world.started.elapsed().as_secs_f32() * 60.0) as usize % n;
            gu.sprite("anc     kaipage", frame, lx + gu.text_width(&last) + 2.0, ly + 3.0, 0x8080_8080);
        }
        // FUN_1015ad10: the channel caption (windowps by chat mode; Say by default) at (x0 + textw - 352, y0).
        if log.lines > 0 {
            gu.sprite("menu    windowps", logwin::caption(world.chat_prefix.split(' ').next().unwrap_or("")), x + tw - 352.0, y, 0x8080_8080);
        }
        blockers.push(log.rect);
    }
    // The chat line ("menu    inline") (16, H - 38), (textw + 14) x 22, windowps 37; FUN_101b7fd0.
    if world.typing {
        let (x, y, w) = (16.0, sh - 38.0, tw + 14.0);
        gu.frame(x, y, w, 22.0);
        gu.sprite_from("menu    windowps", 37, 1, x, y, 0x8080_8080);
        // The channel tab on the line's top edge, by the line's prefix or the chat mode; the linkshell or Unity after it.
        let mode = chatctrl::line_mode(&world.chat_input, world.chat_mode);
        if let Some(k) = chatctrl::tab_sprite(mode) {
            gu.sprite("menu    fep", k, x, y - 12.0, 0x8080_8080);
            match mode {
                6 => chatctrl::ls_line(gu, world, 1, x + 66.0, y - 12.0),
                0xc => chatctrl::ls_line(gu, world, 2, x + 66.0, y - 12.0),
                0xb => chatctrl::unity_line(gu, world, x + 66.0, y - 12.0),
                _ => {}
            }
            blockers.push([x, y - 14.0, 64.0, 14.0]);
        }
        // Phrases in the 7F 69 colour, without brackets (FUN_101b5470); the line's tail when it is too long.
        // A long line keeps the caret in view: the text before it loses chars at the left, the text after it is cut at
        // the right (inf: retail's scroll is not traced; as the event input window).
        let at = fep::caret(world);
        let mut before = fep::pieces(world, &world.chat_input.chars().take(at).collect::<String>());
        let after = fep::pieces(world, &world.chat_input.chars().skip(at).collect::<String>());
        // FUN_10190280 cuts the line to inline +0x34 = the log's text width - 86.
        let room = tw - 86.0;
        let width = |p: &[(String, bool)]| p.iter().map(|(t, _)| gu.text_width(t)).sum::<f32>();
        while width(&before) > room && !before.is_empty() {
            let t = &mut before[0].0;
            let mut c = t.chars();
            c.next();
            *t = c.as_str().to_string();
            if t.is_empty() {
                before.remove(0);
            }
        }
        let mut left = room - width(&before);
        let mut pieces = before.clone();
        for (t, phrase) in after {
            let mut cut = String::new();
            for ch in t.chars() {
                let cw = gu.text_width(&ch.to_string());
                if cw > left {
                    left = -1.0;
                    break;
                }
                left -= cw;
                cut.push(ch);
            }
            if !cut.is_empty() {
                pieces.push((cut, phrase));
            }
            if left < 0.0 {
                break;
            }
        }
        // FUN_101b8660 at (x0 + 2, y0 + 3).
        let mut cx = x + 2.0;
        let caret_x = cx + width(&before);
        // Our selection: a box behind the selected text, placed from the prefix widths relative to the caret.
        if let Some((s, e)) = fep::selection(world) {
            let upto = |n: usize| width(&fep::pieces(world, &world.chat_input.chars().take(n).collect::<String>()));
            let full = upto(at);
            let (x0, x1) = (caret_x - (full - upto(s)), caret_x - (full - upto(e)));
            let (x0, x1) = (x0.max(cx), x1.min(cx + room));
            if x1 > x0 {
                gu.rect(x0, y + 3.0, x1 - x0, 16.0, [0.35, 0.5, 0.85, 0.5]);
            }
        }
        let mut word_x = cx;
        // FUN_101b8dc0: 7F 69 is mode 0xcf (a blue), alpha forced to 0x7f.
        let mut phrase_c = world.battle.as_ref().map_or([0x80, 0x50, 0x40, 0x80], |b| b.colour_in(0xcf, &world.game));
        phrase_c[3] = 0x7f;
        for (t, phrase) in &pieces {
            gu.text(t, cx, y + 3.0, 0, if *phrase { phrase_c } else { WHITE });
            if !*phrase {
                word_x = cx + t.rfind(' ').map_or(0.0, |i| gu.text_width(&t[..=i]));
            }
            cx += gu.text_width(t);
        }
        // The caret: a '|' after the text before it, not counted in the width, while inline +0x20 (0..25 a draw at 30
        // fps) is 10..24.
        let phase = (world.started.elapsed().as_secs_f32() * 30.0) as u64 % 26;
        if (10..=24).contains(&phase) {
            gu.text("|", caret_x, y + 3.0, 0, WHITE);
        }
        blockers.push([x, y, w, 22.0]);
        if let Some(f) = &world.fep {
            blockers.push(fep::draw(gu, f, word_x - 8.0, y));
        }
    }
    if let Some(inp) = event::input(world) {
        let text = String::from_utf8_lossy(&inp.text).into_owned();
        blockers.extend(draw_event_input(gu, inp.kind, &text, inp.caret, sw, sh));
    }
    if st.target.is_some() {
        blockers.extend(gu.window_rect("menu    targetwi", sw, sh));
    }
    blockers.extend(moogle::draw(gu, world, sw, sh));
    blockers.extend(post::draw(gu, world, sw, sh));
    blockers.extend(treasure::draw(gu, world, sw, sh));
    treasure::draw_chest(gu, world, sw, sh);
    partymenu::draw_invite_icon(gu, world, sw, sh);
    blockers.extend(shop::draw(gu, world, sw, sh, log.rect[1]));
    blockers.extend(trade::draw(gu, world, sw, sh));
    blockers.extend(gauge::draw(gu, world, sw, sh));
    buffs::draw(gu, world, sw, sh);
    buffs::draw_party(gu, world, sw, sh, log.rect);
    // Layer 2 (scresult) over the status icons' layer 0 (registry +0x29).
    blockers.extend(search::draw(gu, world, sw, sh));
    // arealist draws over scresult (layer 3) and beside searchma / scsibori.
    blockers.extend(arealist::draw(gu, world, sw, sh));
    blockers.extend(seacom::draw(gu, world, sw, sh));
    blockers.extend(region::draw_menu(gu, world, sw, sh));
    blockers.extend(inspect::draw(gu, world, sw, sh));
    blockers.extend(synth::draw(gu, world, sw, sh));
    blockers.extend(items::draw(gu, world, sw, sh));
    // The action menu (FUN_101276a0), beneath the lists it opens: label sprites, the highlighted one tinted 0x80805020; none
    // while it opens; the hand only while it has the focus.
    if let (Some(m), Some((r, rows))) = (world.action_menu.as_ref().filter(|_| !menu_off), &action_rows) {
        let open = gu.window(&combat::window_name(m.items.len()), sw, sh).is_some();
        let focused = world.magic.is_none() && world.ability.is_none() && world.cats.is_none() && world.win.is_none() && world.loot.is_none() && world.chatctrl.is_none();
        for (k, (c, row)) in m.items.iter().zip(rows).enumerate().filter(|_| open) {
            let (lib, sprite) = combat::label(*c);
            gu.sprite(lib, sprite, row[0], row[1], if k == m.sel { 0x8080_5020 } else { 0x8080_8080 });
            if k == m.sel && focused {
                gu.cursor(row[0] - 2.0, row[1]);
            }
        }
        blockers.push(*r);
    }

    // chatctrl over the action menu that opened it.
    blockers.extend(chatctrl::draw(gu, world, sw, sh));
    blockers.extend(partymenu::draw(gu, world, sw, sh));
    blockers.extend(levelsync::draw(gu, world, sw, sh));
    blockers.extend(viewhouse::draw(gu, world, sw, sh));
    blockers.extend(bazaar::draw(gu, world, sw, sh));
    blockers.extend(helpdesk::draw(gu, world, sw, sh));
    blockers.extend(macroedit::draw(gu, world, sw, sh));
    blockers.extend(searchmenu::draw(gu, world, sw, sh));
    blockers.extend(status::draw(gu, world, sw, sh));
    blockers.extend(currencies::draw(gu, world, sw, sh));
    blockers.extend(unity::draw(gu, world, sw, sh));
    blockers.extend(merits::draw(gu, world, sw, sh));
    blockers.extend(equipwin::pup_draw(gu, world, sw, sh));
    blockers.extend(jobpoints::draw(gu, world, sw, sh));
    blockers.extend(aepoints::draw(gu, world, sw, sh));
    blockers.extend(missions::draw(gu, world, sw, sh));
    blockers.extend(quests::draw(gu, world, sw, sh));
    blockers.extend(keyitems::draw(gu, world, sw, sh));
    blockers.extend(linkshell::draw(gu, world, sw, sh));
    blockers.extend(linklist::draw(gu, world, sw, sh));
    blockers.extend(blklist::draw(gu, world, sw, sh));
    // The item info window over the action menu: both sit on the log (flag 0x1000), and it opens later (raise order).
    blockers.extend(draw_item_info(gu, world, sw, sh, log.rect[1]));
    blockers.extend(magic::draw(gu, world, sw, sh));
    blockers.extend(bluset::draw(gu, world, sw, sh));
    magic::draw_cast_bar(gu, world, sw, sh);
    if let Some(c) = world.config.as_mut() {
        c.covered = world.settings.is_some();
    }
    if let Some(c) = &world.config {
        blockers.extend(c.draw(gu, &world.game, sw, sh));
    }
    if let Some(s) = &world.settings {
        blockers.push(s.draw(gu, sw, sh));
    }
    // The map covers the screen (mapframe takes the mouse), as does cnqframe.
    if world.map.is_some() || world.cnq.is_some() {
        blockers.push([0.0, 0.0, sw, sh]);
    }
    // The choice menu (FUN_100b0960): question at the top, three option rows with the anc_s hand.
    if let Some(c) = event::choice(world)
        && let Some((qx, qy)) = gu.window("menu    query", sw, sh)
    {
        let (question, options, scroll, cursor) = (c.question.clone(), c.options.clone(), c.scroll, c.cursor);
        for (k, l) in question.split('\n').enumerate() {
            gu.text(l, qx + 8.0, qy + 6.0 + k as f32 * 16.0, 0, WHITE);
        }
        let rows = gu.children("menu    query");
        for (k, (row, (_, opt))) in rows.iter().zip(options.iter().skip(scroll)).enumerate() {
            gu.text(opt, qx + row[0] + 4.0, qy + row[1], 0, WHITE);
            if scroll + k == cursor {
                gu.cursor(qx + row[0], qy + row[1]);
            }
        }
        // FUN_1014de10: 3 rows, total the options, top the first shown (FUN_1014e300); at x1 - 8 when stacked.
        if let Some([fx, fy, fw, fh]) = gu.window_rect("menu    query", sw, sh) {
            gu.scroll_bar(fx + fw - 8.0, fy, fh, options.len(), rows.len(), scroll);
            gu.bar_site(scrollbar::Owner::Query, fx + fw - 8.0, fy, fh, options.len(), rows.len(), scroll);
        }
        blockers.extend(gu.window_rect("menu    query", sw, sh));
    }
    if world.menu.is_some() {
        blockers.extend(gu.window_rect(crate::render::screens::menu_window(world.menu_page), sw, sh));
    }
    // FUN_101d62b0: the door menu, title group 9 0x45 and its rows 0x14 to the right of child n.
    if let Some(m) = &world.mogdoor
        && let Some((wx, wy)) = gu.window(myroom::WINDOW, sw, sh)
    {
        let kids = gu.children(myroom::WINDOW);
        let g9 = |id: usize| world.battle.as_ref().and_then(|b| b.group9(id)).unwrap_or_default();
        gu.text(&g9(0x45), wx + 3.0, wy + 3.0, 0, WHITE);
        let rows: Vec<(usize, String, [u8; 4])> = if m.list {
            let names = world.names.clone();
            myroom::areas(m.bit)
                .iter()
                .enumerate()
                .map(|(k, &z)| {
                    let name = names.as_ref().and_then(|n| crate::dat::names::NameSource::place(&**n, z as u32)).unwrap_or_default();
                    let c = if z == 0x118 && !m.garden { [0x40, 0x20, 0x40, 0x80] } else { WHITE };
                    (k, name, c)
                })
                .collect()
        } else {
            myroom::ROWS
                .iter()
                .enumerate()
                .map(|(k, &(child, id))| {
                    let id = if k == 1 && world.mh_owner != 0 { 0xf1 } else { id };
                    let c = if k == 2 && !m.floors { [0x40, 0x40, 0x40, 0x80] } else { WHITE };
                    (child as usize - 1, g9(id), c)
                })
                .collect()
        };
        for (k, (child, text, c)) in rows.iter().enumerate() {
            let Some(r) = kids.get(*child) else { continue };
            let (x, y) = (wx + r[0] + 20.0, wy + r[1]);
            gu.text(text, x, y, 0, *c);
            if k == m.sel {
                gu.cursor(x - 2.0, y);
            }
        }
        blockers.extend(gu.window_rect(myroom::WINDOW, sw, sh));
    }

    // The macro palette (FUN_101a3b20): mcr1pall / mcr2pall (mcr?long when config 0x9d is 0 and the UI is 1024 or
    // wider), the keycap cells, each cell's macro title: letters and digits in the mcralpha sprites, anything else in
    // the UI font.
    if world.palette.0 != 0
        && let Some(m) = world.macros.as_ref()
    {
        let p = world.palette.0 as usize;
        let long = world.game.get(0x9d) == 0 && sw >= 1024.0;
        let name = format!("menu    mcr{p}{}", if long { "long" } else { "pall" });
        // 0x82 to the right while gaugewin or cnttime is up (FUN_101a3840, mode DAT_106218ee 0).
        let dx = macroedit::palette_dx(world);
        if let Some(win) = gu.lobby_window_off(&name, sw, sh, (dx, 0.0), Some(world.palette.3[p - 1] as u16 + 1), true, &[]) {
            let pad = if win.frame[2] >= 800.0 { 4.0 } else { 2.0 };
            for (k, (_, r)) in win.children.iter().take(10).enumerate() {
                macroedit::cell_title(gu, &m.recs[k + 10 * (p - 1)].title(), *r, pad);
            }
            blockers.push(win.frame);
        }
    }
    // The yes / no prompt (FUN_10210e30): "menu    comyn" stacked on the log (registry flag 0x1000: frame top = log top
    // - 88 - 2, at least 16), the question at (+4, +4) wrapped to w - 8 (FUN_10211050), Yes / No children 1 / 2.
    if let Some((kind, yes)) = world.confirm {
        let on_yes = world.confirm.map_or(yes, |c| c.1);
        let layout = if matches!(kind, Confirm::Merit(_) | Confirm::JobPoint(_)) { "menu    merityn" } else { "menu    comyn" };
        let mut dy = gu.window_rect(layout, sw, sh).map_or(0.0, |r| (log.rect[1] - r[3] - 2.0).max(16.0) - r[1]);
        // FUN_10232770's prompts sit under quest01 (FUN_10119110: bottom + 2).
        if let Confirm::Roe(a) = kind
            && roe::under_list(a)
            && let (Some(q), Some(r)) = (gu.window_rect(quests::LIST, sw, sh), gu.window_rect(layout, sw, sh))
        {
            dy = q[1] + q[3] + 2.0 - r[1];
        }
        let msg = confirm_msg(gu, world, kind);
        if let Some(win) = gu.lobby_window_off(layout, sw, sh, (0.0, dy), Some(if on_yes { 1 } else { 2 }), true, &[]) {
            let [fx, fy, fw, _] = win.frame;
            for (i, line) in gu.wrap(&msg, fw - 8.0).iter().enumerate() {
                gu.text(line, fx + 4.0, fy + 4.0 + 16.0 * i as f32, 0, WHITE);
            }
            for (id, r) in win.children.iter().filter(|(id, _)| matches!(id, 1 | 2)) {
                if input.pointer.is_some_and(|p| inside(r, p)) {
                    gu.hover.set(true);
                    if input.moved || input.clicked {
                        world.confirm = Some((kind, *id == 1));
                    }
                    if input.clicked {
                        answer = Some(*id == 1);
                    }
                }
            }
            blockers.push(win.frame);
        }
    }
    for t in &slots {
        match t.what {
            "target name" => {
                // FUN_101501a0: FUN_101c9fa0(W - 13, L + 5, T + 6, name, the name-plate colour, centred), split into
                // lines only while the target has no gauge.
                if let Some(e) = target {
                    let max = gu.window_rect("menu    targetwi", sw, sh).map_or(100.0, |r| r[2] - 13.0);
                    let tid = world.target.unwrap_or(0);
                    let tint = name_colour(tid, e, world, |i| gu.ncol(i));
                    gu.wrapped_name(&e.name, max, t.x, t.y, tint, true, e.no_gauge);
                }
            }
            // Alliance rows (FUN_10202720) by their window slot: name (TP as on party rows), HP in the HP colour, MP,
            // out of zone the name alone in 0x80606060.
            "ally name" | "ally HP" | "ally MP" | "ally away" => {
                let Some(m) = party_slots.slots.get(t.align as usize).and_then(|m| m.as_ref()) else { continue };
                match t.what {
                    "ally name" => {
                        gu.shape_name(&m.name, t.x, t.y, 0x8080_8080);
                        if world.party_tp && m.index < 0x700 {
                            gu.shape_text(&m.tp.to_string(), t.x + 48.0, t.y + 10.0, if m.tp > 999 { 0x8020_60d0 } else { 0x8060_6060 }, true);
                        }
                    }
                    "ally HP" => {
                        let tint = gu.hp_tint(m.hpp);
                        gu.shape_text(&m.hp.to_string(), t.x, t.y, tint, true);
                    }
                    "ally MP" => {
                        gu.shape_text(&m.mp.to_string(), t.x, t.y, 0x8080_8080, true);
                    }
                    _ => gu.shape_name(&m.name, t.x, t.y, 0x8060_6060),
                }
            }
            "member name" | "HP" | "MP" | "member away" => {
                let row = t.align as usize;
                // Ours from our own vitals; the others from the roster (name, HP / MP / TP, HPP).
                let v = match rows.get(row) {
                    Some(None) => world.vitals.map(|(hp, mp, tp, hpp, _)| (world.name.clone(), hp, mp, tp, hpp, 0u16, row)),
                    Some(Some(m)) => Some((m.name.clone(), m.hp, m.mp, m.tp, m.hpp, m.zone, row)),
                    None => None,
                };
                let Some((name, hp, mp, tp, hpp, zone, row)) = v else { continue };
                match t.what {
                    "member name" => {
                        gu.shape_name(&name, t.x, t.y, 0x8080_8080);
                        // TP (FUN_101f9940): right-aligned at (+0x30, +10), blue from 1000; with config 0xae bit 0
                        // (ours `party_tp`), except trust rows (FUN_10201eb0: ActIndex 0x700 and up).
                        let trust = rows.get(row).and_then(|m| m.as_ref()).is_some_and(|m| m.index >= 0x700);
                        if world.party_tp && !trust {
                            gu.shape_text(&tp.to_string(), t.x + 48.0, t.y + 10.0, if tp > 999 { 0x8020_60d0 } else { 0x8060_6060 }, true);
                        }
                    }
                    "HP" => {
                        let tint = gu.hp_tint(hpp);
                        gu.shape_text(&hp.to_string(), t.x, t.y, tint, true);
                    }
                    "MP" => {
                        gu.shape_text(&mp.to_string(), t.x, t.y, 0x8080_8080, true);
                    }
                    _ => {
                        // FUN_10202530 out of zone: the name and "(zone)" right-aligned at (+0x69, +0xb), 0x80606060.
                        gu.shape_name(&name, t.x, t.y, 0x8060_6060);
                        let zn = crate::dat::font::decode_sjis(&gu.area_name(zone));
                        gu.shape_text(&format!("({zn})"), t.x + 105.0, t.y + 11.0, 0x8060_6060, true);
                    }
                }
            }
            _ => {}
        }
    }
    // The party window's hand over the log's edge (partywin layer 3 above logwindo's 1).
    gu.party_hand(sw, sh, &st);

    // The K.O. window ("menu    dead", FUN_1012def0): "Time Left:" baked in its frame sprite, the time "%d:%02d" (minutes
    // unwrapped) at (+75, +6) in the sprite font, child 1 "Back to Home Point" with the hand while it has the focus.
    if world.ko_open
        && let Some(win) = gu.lobby_window_off("menu    dead", sw, sh, (0.0, 0.0), Some(1), world.confirm.is_none() && world.ko_offer != 2, &[])
    {
        let s = world.ko_deadline.saturating_sub(vana_now(world));
        gu.shape_text(&format!("{}:{:02}", s / 60, s % 60), win.frame[0] + 75.0, win.frame[1] + 6.0, 0x8080_8080, false);
        for (id, r) in win.children.iter().filter(|(id, _)| *id == 1) {
            if world.confirm.is_none() && world.ko_offer != 2 && input.pointer.is_some_and(|p| inside(r, p)) {
                gu.hover.set(true);
                if input.clicked && *id == 1 {
                    world.confirm = Some((Confirm::HomePoint, false));
                }
            }
        }
        blockers.push(win.frame);
    }
    // The raise prompt "menu    resynus" (FUN_10210da0, registry flag 0x1000: stacked on the log like comyn).
    // Stacked by lobby_window_off itself. The hover runs every frame (FUN_10125360: the cursor moves when the child
    // under the pointer changes, SE 0001); the release confirms the cursor child (FUN_1015de50, SE 0002).
    if dead && world.ko_offer == 2 && world.confirm.is_none() {
        if let Some(win) = gu.lobby_window_off("menu    resynus", sw, sh, (0.0, 0.0), Some(if world.raise_yes { 1 } else { 2 }), true, &[]) {
            let hovered = input.pointer.and_then(|p| win.children.iter().find(|(id, r)| matches!(id, 1 | 2) && inside(r, p)).map(|(id, _)| *id));
            if hovered.is_some() {
                gu.hover.set(true);
            }
            if let Some(id) = hovered.filter(|&id| Some(id) != world.raise_hover && (id == 1) != world.raise_yes) {
                world.raise_yes = id == 1;
                world.sounds.push(crate::audio::Ui::Cursor as u32);
            }
            world.raise_hover = hovered;
            if input.clicked
                && let Some(id) = hovered
            {
                world.raise_yes = id == 1;
                answer_raise(world, id == 1);
                world.sounds.push(crate::audio::Ui::Select as u32);
            }
            blockers.push(win.frame);
        }
    } else {
        world.raise_hover = None;
    }
    match (answer, world.confirm) {
        // LAB_1012e0a0: Yes sets +0x24; the state machine sends 0x01A 0x0B.
        (Some(true), Some((Confirm::HomePoint, _))) => {
            world.confirm = None;
            world.homing = true;
        }
        // LAB_10097390: the tractor answer, 0x01A 0x13 (0 accept, 1 reject).
        (Some(yes), Some((Confirm::Tractor, _))) => {
            world.confirm = None;
            world.ko_offer = 0;
            send_menu_answer(world, 0x13, !yes as u32);
        }
        // FUN_101f3040 / FUN_101f3080: the Linkshell asks twice more; then FUN_101f2ff0 throws the stack away.
        (Some(true), Some((Confirm::Drop { index, id, stage }, _))) => {
            if id == 0x201 && stage < 2 {
                world.confirm = Some((Confirm::Drop { index, id, stage: stage + 1 }, false));
            } else {
                world.confirm = None;
                items::drop_now(world, index);
            }
        }
        (Some(true), Some((Confirm::Sync { id, off, .. }, _))) => {
            world.confirm = None;
            levelsync::answer(world, id, off);
        }
        (Some(true), Some((Confirm::Ls(a), _))) => {
            world.confirm = None;
            linkshell::answer(world, a);
        }
        (Some(true), Some((Confirm::Macro(a), _))) => {
            world.confirm = None;
            macroedit::answer(world, a);
        }
        (Some(true), Some((Confirm::LinkKick(k), _))) => {
            world.confirm = None;
            linklist::kick(world, &k);
        }
        (Some(true), Some((Confirm::Merit(a), _))) => {
            world.confirm = None;
            merits::answer(world, a);
        }
        (Some(true), Some((Confirm::JobPoint(a), _))) => {
            world.confirm = None;
            jobpoints::answer(world, a);
        }
        (Some(true), Some((Confirm::AlterEgo(a), _))) => {
            world.confirm = None;
            aepoints::answer(world, a);
        }
        (Some(true), Some((Confirm::BlueRemoveAll, _))) => {
            world.confirm = None;
            bluset::remove_all(world);
        }
        (Some(true), Some((Confirm::Roe(a), _))) => {
            world.confirm = None;
            roe::answer(world, a);
        }
        (Some(yes), Some((Confirm::Request(a), _))) => {
            world.confirm = None;
            partyreq::answer(world, a, yes);
        }
        (Some(true), Some((Confirm::NewAdventurer, _))) => {
            world.confirm = None;
            helpdesk::new_adventurer_off(world);
        }
        (Some(true), Some((Confirm::CancelTrade, _))) => {
            world.confirm = None;
            trade::cancel_request(world);
        }
        (Some(true), Some((Confirm::Post(ctx), _))) => {
            world.confirm = None;
            post::confirmed(world, ctx);
        }
        // LAB_1012bca0: /dismount.
        (Some(true), Some((Confirm::Dismount, _))) => {
            world.confirm = None;
            textcmd::run(world, "/dismount", textcmd::Mode::Internal);
        }
        // LAB_101f7d40: c2s 0x0F6, the ring goes, scanlist opens.
        (Some(true), Some((Confirm::StopTracking, _))) => {
            world.confirm = None;
            let _ = world.net.commands.send(NetCommand::Packet { id: 0x0F6, body: vec![0; 4] });
            world.scan.stop();
            if let Some(m) = world.map.as_mut() {
                m.after_stop();
            }
        }
        (Some(true), Some((Confirm::LeaveRoom, _))) => {
            world.confirm = None;
            leave_room(world, 0);
        }
        (Some(true), Some((kind, _))) => {
            world.confirm = None;
            // LAB_10159f00 / LAB_10159ee0: Yes runs "/logout" or "/shutdown" again (a toggle, which also cancels).
            send_chat(world, if kind == Confirm::Shutdown { "/shutdown".into() } else { "/logout".into() });
        }
        (Some(false), _) => world.confirm = None,
        _ => {}
    }

    blockers
}

/// What a lobycwin change needs from the app (FUN_10196390): title music 0xaf, volume 0xa8 / aspect 0xab.
#[derive(Default)]
struct LobbyApply {
    music: bool,
    volume: bool,
}

fn lobby_changes(ids: &[u16], a: &mut LobbyApply) {
    for id in ids {
        match id {
            0xaf => a.music = true,
            0xa8 | 0xab => a.volume = true,
            _ => {}
        }
    }
}

/// Takes a Config step: its sounds, the changed ids' effects, closing.
fn config_out(world: &mut World, out: configui::Out) {
    world.sounds.extend(out.sounds.iter().map(|s| *s as u32));
    for id in out.changed {
        cfgapply::apply(world, id);
    }
    if out.closed {
        world.config = None;
        world.game_save = true;
    }
    if out.open_settings {
        world.want_settings = Some(settings::Kind::Settings);
    }
    if out.open_extras {
        world.want_settings = Some(settings::Kind::Extras);
    }
    if let Some(mode) = out.lockstyle {
        equipsets::lock_mode(world, mode, false, 0);
    }
}

/// The indicator's draw `n` (from 1).
pub fn downloading(gu: &mut GameUi, n: usize, sw: f32, sh: f32) {
    let dy = (2 * n).min(8) as f32;
    gu.sprite_scaled("menu    keytops3", 165 + (n >> 2) % 5, sw - 234.0, sh - 48.0 + 8.0 - dy, 1.0, (n as f32 * 0.25).min(1.0), 0x8080_8080);
}

/// Logs a zone without a group 0x14 name once per zone.
fn missing_place(zone: u16) {
    static LAST: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(u32::MAX);
    if LAST.swap(zone as u32, std::sync::atomic::Ordering::Relaxed) != zone as u32 {
        log::error!("no place name (group 0x14) for zone {zone}");
    }
}

/// The top bars' text slots. Slot 1: the focused window's cursor item or header (FUN_10115380). Slot 0: the
/// action menu, the command's help and "Battle Mode" (engaged) or "Commands" (group 0xd 7 / 5; 6 "Ready Mode"
/// is for a FUN_101c26a0 target, not modelled), FUN_10127790.
/// Mode 6's cursor point height (FUN_101f9a50): midway between helpwind's bottom and the top of the menu, else of the
/// log (C integer division).
fn preview_point_y(gu: &GameUi, world: &World, sw: f32, sh: f32) -> f32 {
    let h = 16 + 30;
    let menu = event::choice(world).and_then(|_| gu.window_rect("menu    query", sw, sh)).map(|r| r[1]);
    let t = menu.or(gu.log_top).unwrap_or(sh) as i32;
    (h + (t - h) / 2) as f32
}

fn bar_text(gu: &GameUi, world: &World, items: &[(usize, [f32; 4])]) -> hudtop::BarText {
    let mut bars = hudtop::BarText::default();
    let map0 = world.map.as_ref().and_then(|m| m.focus());
    let cfg_focus = world.config.as_ref().and_then(|c| c.focus());
    let focus = cfg_focus.as_ref().map(|(n, c)| (n.as_str(), *c)).or(items::focus(world)).or(map0).or_else(|| world.menu.and_then(|k| items.get(k)).map(|(i, _)| (crate::render::screens::menu_window(world.menu_page), *i as u16 + 1)));
    // mapframe's base draw (FUN_101eebd0, +0xb4) puts the map's place name (FUN_101cbc10(zone, 1)) in helpwind slot 2.
    if let Some(zone) = world.map.as_ref().and_then(|m| m.help_zone())
        && let Some(names) = world.names.as_ref()
    {
        match crate::dat::names::NameSource::place(&**names, zone as u32).filter(|n| !n.is_empty()) {
            Some(n) => bars.help[2] = n,
            None => missing_place(zone),
        }
    }
    // A prompt (comyn) has the focus, and its items carry no text.
    if let Some((name, child)) = focus.filter(|_| world.confirm.is_none()) {
        let [help, title] = gu.focus_text(name, child);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
        if let Some(h) = configui::our_help(child) {
            bars.help[1] = h.into();
        }
    }
    // FUN_10140090 / FUN_10140270: row 12 in the Mog House: title 0xd 9 "Mog House", help 0xe 0x29 (own house) or 0x2a.
    if world.myroom.is_some()
        && world.confirm.is_none()
        && focus == Some((crate::render::screens::menu_window(0), 12))
    {
        bars.title[1] = gu.title_msg(9).unwrap_or_default();
        bars.help[1] = gu.menu_msg(if world.mh_owner == 0 { 0x29 } else { 0x2a }, "");
    }
    if let Some(s) = &world.settings {
        (bars.help[1], bars.title[1]) = (s.help(), s.title().into());
    }
    // The palette's caption (FUN_101a3d00): group 10 0x27 "Macro Set %s (#%d)"; titles "MacroPalette" (group 0xd
    // 0x50) under the cursor cell's macro title.
    if world.palette.0 != 0
        && let Some(m) = world.macros.as_ref()
    {
        let p = world.palette.0 as usize - 1;
        let fmt = gu.lobby_msg(0x0a, 0x27, "Macro Set %s (#%d)");
        bars.help[1] = fmt.replacen("%s", &m.book_title(m.book), 1).replacen("%d", &(m.set + 1).to_string(), 1);
        bars.title[1] = gu.title_msg(0x50).unwrap_or_else(|| "MacroPalette".into());
        bars.title[2] = m.recs[world.palette.3[p] + 10 * p].title();
    }
    // The raise prompt's rows: help 0xe 407 / 408, title 0xd 101 "Revival" from its children's strings.
    if world.ko_offer == 2 && world.confirm.is_none() {
        let [help, title] = gu.focus_text("menu    resynus", if world.raise_yes { 1 } else { 2 });
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    } else if world.ko_open && world.confirm.is_none() {
        // The K.O. window's button: help group 0xe 0xf1, title 0xd 0x39 "K.O." from child 1's strings.
        let [help, title] = gu.focus_text("menu    dead", 1);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    }
    magic::bars(gu, world, &mut bars);
    bluset::bars(gu, world, &mut bars);
    moogle::bars(gu, world, &mut bars);
    post::bars(gu, world, &mut bars);
    treasure::bars(gu, world, &mut bars);
    chatctrl::bars(gu, world, &mut bars);
    searchmenu::bars(gu, world, &mut bars);
    partymenu::bars(gu, world, &mut bars);
    levelsync::bars(gu, world, &mut bars);
    viewhouse::bars(gu, world, &mut bars);
    bazaar::bars(gu, world, &mut bars);
    helpdesk::bars(gu, world, &mut bars);
    macroedit::bars(gu, world, &mut bars);
    status::bars(gu, world, &mut bars);
    currencies::bars(gu, world, &mut bars);
    unity::bars(gu, world, &mut bars);
    merits::bars(gu, world, &mut bars);
    equipwin::pup_bars(gu, world, &mut bars);
    jobpoints::bars(gu, world, &mut bars);
    aepoints::bars(gu, world, &mut bars);
    missions::bars(gu, world, &mut bars);
    quests::bars(gu, world, &mut bars);
    keyitems::bars(gu, world, &mut bars);
    linkshell::bars(gu, world, &mut bars);
    linklist::bars(gu, world, &mut bars);
    blklist::bars(gu, world, &mut bars);
    shop::bars(gu, world, &mut bars);
    search::bars(gu, world, &mut bars);
    arealist::bars(gu, world, &mut bars);
    seacom::bars(gu, world, &mut bars);
    region::bars(gu, world, &mut bars);
    inspect::bars(gu, world, &mut bars);
    trade::bars(gu, world, &mut bars);
    synth::bars(gu, world, &mut bars);
    items::bars(gu, world, &mut bars);
    buffs::bars(gu, world, &mut bars);
    if let Some(m) = &world.action_menu {
        bars.help[0] = match m.items.get(m.sel) {
            // FUN_1012b4b0: Treasure's help is 0x1b "Distribute ..." in a party, else 0x1c "Acquire ...".
            Some(&combat::cmd::TREASURE) if world.party.members().count() <= 1 => gu.menu_msg(0x1c, ""),
            Some(&c) => gu.cmd_help(c).unwrap_or_default(),
            None => String::new(),
        };
        // Retail shows group 0xd 5 "Commands" while engaged too (seen live, 2026-10-24), though FUN_1012b4b0 reads as 7
        // "Battle Mode" when +0x170 == 1: that field is not the engaged status (inferred).
        bars.title[0] = gu.title_msg(5).unwrap_or_default();
    }
    bars
}

/// The yes / no prompt's question: group 0xe (log out, shut down, or cancelling either) or the Mog House door's.
fn confirm_msg(gu: &GameUi, world: &World, kind: Confirm) -> String {
    match (kind, world.logging_out) {
        (Confirm::Logout, false) => gu.menu_msg(0x48, "Log out?"),
        (Confirm::Logout, true) => gu.menu_msg(0x49, "Cancel current logout process?"),
        (Confirm::Shutdown, false) => gu.menu_msg(0x277, "Shut down?"),
        (Confirm::Shutdown, true) => gu.menu_msg(0x278, "Cancel shut down process?"),
        (Confirm::LeaveRoom, _) if myroom::in_city(world.zone) => gu.lobby_msg(0x0c, 0x2e, "Moogle: Heading outside, kupo?"),
        (Confirm::LeaveRoom, _) => gu.lobby_msg(0x0c, 0x2f, "Exit your Mog House?"),
        (Confirm::HomePoint, _) => gu.menu_msg(0x46, "Return to Home Point?"),
        (Confirm::Tractor, _) => gu.menu_msg(0x60, "Accept Tractor?"),
        (Confirm::StopTracking, _) => gu.menu_msg(0x45, "Stop tracking current target\nand proceed with wide area scan?"),
        (Confirm::Dismount, _) => gu.menu_msg(0x4d, "Dismount?"),
        (Confirm::Post(ctx), _) => post::confirm_text(gu, ctx),
        (Confirm::CancelTrade, _) => gu.lobby_msg(0x0c, 0x2d, "Cancel trade transaction?"),
        (Confirm::Ls(a), _) => linkshell::ask_text(gu, a),
        (Confirm::Sync { id, race, off }, _) => levelsync::ask_text(world, id, race, off),
        (Confirm::NewAdventurer, _) => world.battle.as_ref().and_then(|b| b.group9(0xd4)).unwrap_or_default(),
        (Confirm::Macro(a), _) => macroedit::ask_text(world, a),
        (Confirm::Merit(a), _) => merits::ask_text(world, a),
        (Confirm::LinkKick(k), _) => linklist::kick_text(world, &k),
        (Confirm::JobPoint(a), _) => jobpoints::ask_text(world, a),
        (Confirm::AlterEgo(a), _) => aepoints::ask_text(world, a),
        (Confirm::Request(_), _) => partyreq::ask_text(world),
        (Confirm::Roe(a), _) => roe::ask_text(gu, a),
        (Confirm::BlueRemoveAll, _) => world.battle.as_ref().and_then(|b| b.group9(0xe4)).unwrap_or_default(),
        (Confirm::Drop { id, stage: 0, .. }, _) => {
            let name = world.item_info.as_ref().and_then(|c| c.item(id)).map(|it| it.name.clone()).unwrap_or_default();
            gu.lobby_msg(0x0a, 0x30, "Really dispose of the %s?").replacen("%s", &name, 1)
        }
        (Confirm::Drop { stage, .. }, _) => gu.item_msg(9, if stage == 1 { 0x157 } else { 0x158 }).unwrap_or_default(),
    }
}

/// LAB_101d6040: the door opens the exit menu when the house has an exit bit (FUN_10097bc0), else the prompt.
fn open_mogdoor(w: &mut World) {
    let Some(r) = w.myroom else { return };
    if w.leaving_room {
        return;
    }
    let bit = myroom::exit_bit(&r);
    if bit == 0 {
        w.confirm = Some((Confirm::LeaveRoom, false));
        return;
    }
    // +0x1a: 2F unlocked (s2c 0x067 @0x27 bit 0); +0x19 Mog Garden: key item 0x86d; both only in our own house
    // (FUN_101cc0a0: 0x067 @0x20 owner 0).
    let own = w.mh_owner == 0;
    let floors = myroom::in_city(w.zone) && own && myroom::has_floors(&r) && w.mh_floors;
    let garden = own && w.has_key_item(0x86d);
    w.mogdoor = Some(myroom::Menu { bit, sel: 0, list: false, floors, garden, floor: r.floor });
    w.action_menu = None;
}

/// FUN_101d6540 on a choice.
fn mogdoor_pick(w: &mut World) {
    let Some(m) = w.mogdoor.as_mut() else { return };
    match m.pick() {
        myroom::Pick::Close => w.mogdoor = None,
        myroom::Pick::Exit(mode) => {
            w.mogdoor = None;
            leave_room(w, mode);
        }
        myroom::Pick::Refused(Some(id)) => {
            if let Some(line) = w.battle.as_ref().and_then(|b| b.group9(id)) {
                w.chat.push(line);
            }
        }
        myroom::Pick::Refused(None) | myroom::Pick::None => {}
    }
}

/// FUN_1008aec0: c2s 0x05E 'zmrq' with the exit bit and mode; LSB re-zones us *(inf: the door's opening swing before
/// it is not played yet)*.
fn leave_room(w: &mut World, mode: u8) {
    let Some(r) = w.myroom else { return };
    w.leaving_room = true;
    let _ = w.net.commands.send(NetCommand::MyRoomExit { bit: myroom::exit_bit(&r), mode });
}

/// FUN_101d2cc0 / FUN_101d2ff0: the client's own door actor, at the room's RID record `_720`.
fn place_mogdoor(w: &mut World) {
    if w.myroom.is_none() || !w.in_scene() || w.entities.contains_key(&myroom::DOOR_ID) {
        return;
    }
    let Some(r) = w.sub.regions.iter().find(|r| r.name == myroom::DOOR_RECT) else { return };
    let pos = Vec3::from(r.pos);
    w.entities.insert(
        myroom::DOOR_ID,
        Entity {
            name: myroom::DOOR_NAME.into(),
            pos,
            kind: EntityKind::Npc,
            hpp: 100,
            dir: 0,
            target: pos,
            motion: Default::default(),
            ratio: 0.0,
            last_pos: pos,
            claim: 0,
            mob: false,
            index: myroom::DOOR_INDEX,
            named: false,
            plural: false,
            slide: pos,
            remain: 0.0,
            counter: 0,
            yaw: 0.0,
            dir_seen: 0,
            grounded: false,
            flags: 0,
            no_gauge: false,
            // Kind 3 makes an XiDoorActor (FUN_1008f740), whose plate draw (vtable +0x284, FUN_100ac380) is empty.
            no_plate: true,
            gens_off: false,
            untargetable: false,
            invisible: false,
            radar_off: false,
            plate: net::PlateExtra::default(),
            sync: 0,
            monstrosity: 0,
            sub: [0, 8, 0],
            master: 0,
            init_done: false,
            door: 0,
            lift: None,
            active: 0.0,
            no_bump: false,
            model: 0,
            object: true,
            face: 0,
            spawn_fx: false,
            ride: Default::default(),
            name_ncol: 0x17,
            marks: net::Marks::default(),
            female: false,
            sub_kind: 3,
        },
    );
}

/// Placed narration (rem4line opened by FUN_10131c10 at the FUN_10132030 place: ops 0x67 / 0x68, text codes
/// 02 / 03): the open message's current page without a frame. Other event text is in the log; rem4line's overflow
/// case (FUN_10131d20: the log full and its oldest row the message's) is not modelled. Returns its rect.
fn event_window(gu: &mut GameUi, world: &World, sw: f32, sh: f32, tw: f32) -> Option<[f32; 4]> {
    let (x, y) = event::placement(world, sw, sh)?;
    let waits = event::prompt(world);
    let m = event::message_ref(world)?;
    let page = m.pages.as_ref().and_then(|(_, p)| p.get(m.page).cloned()).unwrap_or_default();
    let (w, h) = (tw + 14.0, page.len().max(1) as f32 * 16.0 + 4.0);
    for (k, line) in page.iter().enumerate() {
        gu.text(line, x + 4.0, y + 2.0 + 16.0 * k as f32, 0, WHITE);
    }
    // FUN_1015ad10: the kaipage arrow after the last line while a key is wanted.
    if waits && let Some(last) = page.last() {
        let n = gu.lib_len("anc     kaipage").max(1);
        let frame = (world.started.elapsed().as_secs_f32() * 60.0) as usize % n;
        let ly = y + 2.0 + 16.0 * (page.len() - 1) as f32;
        gu.sprite("anc     kaipage", frame, x + 4.0 + gu.text_width(last) + 2.0, ly + 3.0, 0x8080_8080);
    }
    Some([x, y, w, h])
}

/// Wraps the open event message to the window width once, in pages of 4 lines (a 5th line makes a keyed page).
fn paginate(gu: &GameUi, world: &mut World, tw: f32) {
    let Some(m) = event::message(world) else { return };
    if m.pages.as_ref().is_some_and(|(w, _)| *w == tw) {
        return;
    }
    let wrapped: Vec<String> = m.lines.iter().flat_map(|l| gu.wrap(l, tw)).collect();
    let mut pages: Vec<Vec<String>> = wrapped.chunks(4).map(<[String]>::to_vec).collect();
    if pages.is_empty() {
        pages.push(Vec::new());
    }
    m.page = m.page.min(pages.len() - 1);
    m.pages = Some((tw, pages));
}

/// A yes/no box (FUN_102075f0 / FUN_10233b50); `yes` is the highlighted answer. Some(answer) once clicked.
fn yes_no(gu: &mut GameUi, input: &UiInput, sw: f32, sh: f32, msg: &str, yes: &mut bool) -> Option<bool> {
    let ww = (gu.text_width(msg) + 48.0).max(200.0);
    let (wx, wy, wh) = (((sw - ww) / 2.0).round(), (sh / 2.0 - 40.0).round(), 56.0);
    gu.frame(wx, wy, ww, wh);
    gu.text(msg, wx + ww / 2.0, wy + 8.0, 1, WHITE);
    let mut answer = None;
    for (k, label) in ["Yes", "No"].iter().enumerate() {
        let x = wx + ww / 2.0 + if k == 0 { -44.0 } else { 20.0 };
        let hit = [x - 4.0, wy + 28.0, gu.text_width(label) + 8.0, 20.0];
        if input.pointer.is_some_and(|p| inside(&hit, p)) {
            gu.hover.set(true);
            if input.moved || input.clicked {
                *yes = k == 0;
            }
            if input.clicked {
                answer = Some(k == 0);
            }
        }
        gu.text(label, x, wy + 30.0, 0, WHITE);
        if (k == 0) == *yes {
            gu.cursor(x - 2.0, wy + 30.0);
        }
    }
    answer
}

/// Nation step backdrop state: the flyover shown (nation, movie), the one loading, the scene brightness
/// (0 = black) and how long the highlight has differed from what is shown.
struct NationShow {
    fly: Option<(usize, flyover::Flyover)>,
    loading: Option<usize>,
    fade: f32,
    hold: f32,
}

impl Default for NationShow {
    fn default() -> Self {
        Self { fly: None, loading: None, fade: 1.0, hold: 0.0 }
    }
}

/// A character drawn with its real model: our own, or another player's.
/// What an actor is drawn from: a player-style look, or an NPC/monster look.
#[derive(Clone, Copy, Debug)]
enum Source {
    Pc(Look),
    Npc(crate::dat::npc::NpcLook),
    /// A self-contained model file (a chocobo or mount, ride.rs).
    File(u32),
}

struct Actor {
    /// Debug text of the source, to notice changes.
    key: String,
    /// What it is drawn from now (event op 0xb6 changes parts of it).
    source: Option<Source>,
    character: Option<Arc<Model>>,
    gpu: Option<crate::render::ActorGpu>,
    /// Motion playing, and ticks (1/60 s, scaled by its rate) since it started.
    motion: String,
    ticks: f32,
    /// The motion being blended out, its ticks and whether it played once, and blend progress in ticks (16 = done).
    prev: Option<(String, f32, bool)>,
    /// The current motion plays once (a one-shot or a single-count scheduler motion): it holds its last frame,
    /// also as the pose a switch blends from (FUN_1001a8e0: regions < 3 hold the last frame when the count ends).
    once: bool,
    blend: f32,
    /// Height of the posed model's top above its feet (world units, y down so negative).
    top: f32,
    /// An attack, damage or death motion played once over the status motion.
    oneshot: Option<combat::OneShot>,
    /// A hit reaction / guard over the motion (regions 5 / 6).
    overlay: Option<actmotion::Overlay>,
    /// The status motion's own clock, kept under a one-shot or scheduler motion (its regions stay below).
    base_ticks: f32,
    /// Event op 0xb6 0xf / 0x17: a scale set by a script (+0x204), over the model's own.
    event_scale: Option<f32>,
    /// Event op 0x59: turn (+0xdc) and head-look (+0xe0) speed percents, used while an event runs; 0 = default.
    turn_pct: i32,
    look_pct: i32,
    /// The attack scheduler used last (FUN_100d46f0 avoids repeating it).
    last_attack: String,
    /// Motion files on top of the model (event banks, battle files), newest last.
    extra: actmotion::Extra,
    /// Effect files whose schedulers play motions from them (Kind::Effect), wanted on `extra`.
    fx_files: Vec<u32>,
    /// `wepN` hide bytes 0 / 1 (a mesh draws while both are clear), and when byte 0 of a timed hide clears.
    wep_hide: [[bool; 2]; 16],
    wep_show_at: [Option<Instant>; 16],
    /// A scheduler's motions (event motions, weapon draw / sheathe), and one waiting for its files.
    play: Option<actmotion::Play>,
    pending: Option<actmotion::Pending>,
    /// Battle files on the list (FUN_100cf650 while engaged) and the engaged state they follow.
    battle: actmotion::Battle,
    /// Blend length of the current switch in ticks (scheduler op 0x05 u16 @0x18, else 16).
    blend_len: f32,
    /// Alpha 0..0x80 (model colour high byte) and a fade toward a target (op 0x6c, "kesu").
    alpha: f32,
    /// The model colour's RGB bytes (0x80 = 1.0), faded by scheduler op 0x29.
    rgb: [f32; 3],
    /// The target flash state (FUN_100c94a0).
    flash: flash::Flash,
    fade: Option<actmotion::Fade>,
    /// FUN_100c56e0 with 'spop': the scheduler to run once the model's files are in (model +0x8c4).
    spop: bool,
    /// Last frame's bone transforms (model space) and model scale, for look points.
    bones: Vec<(glam::Quat, Vec3)>,
    scale: f32,
    /// Set once the rest-pose top is known.
    top_key: String,
    /// Motion name lookups (actmotion::find) by name.
    find_cache: actmotion::FindCache,
    /// The model failed to load (the event actor wait takes it as loaded).
    failed: bool,
    /// Mouth motion of a spoken line, the head look, and the emote scheduler playing (actor +0x1b8).
    mouth: Option<actmotion::Mouth>,
    /// FUN_100cdf90: ticks to the next blink (actor +0x8b8), and ticks into the playing `eye3`.
    blink_wait: f32,
    blink: Option<f32>,
    look: actmotion::Look,
    emote: Option<String>,
    /// Rest-pose bones (model space), for attach points of a look target.
    rest: Vec<(glam::Quat, Vec3)>,
    /// /jump (draw actor +0x8bc): ticks left of the jump motion, below 0 none.
    jump: f32,
    /// The pick's bones (FUN_100cff40): those with geometry whose parent has some too, once per model.
    pick_bones: Option<Vec<u16>>,
    /// Status motion names changed by scheduler ops 0x79 / 0x8c / 0xa4 / 0xa5 (actor +0x7c8..+0x7d4), else the model's.
    motion_names: Option<crate::motion::MotionNames>,
    /// A mount's status motions run at this rate (info[15] / 100, ride.rs).
    rate_scale: f32,
    /// The load fade (+0x59c): 0 for a new actor, up 1/32 a tick once its model (and a rider's animal) is in.
    appear: f32,
    /// The face motion a spoken line plays (entity +0x168, FUN_100846b0: 'mou4', event op 0xae 3 'kut4').
    mouth_name: &'static str,
    /// The ground tilt state (client/tilt.rs) and this frame's tilted rotation (None: the plain Euler one).
    ground_tilt: tilt::GroundTilt,
    tilt_rot: Option<glam::Mat3>,
    /// The riding files on `extra`.
    ride_on: Vec<u32>,
}

impl Actor {
    fn new() -> Self {
        Actor {
            key: String::new(),
            source: None,
            character: None,
            gpu: None,
            motion: String::new(),
            ticks: 0.0,
            prev: None,
            once: false,
            blend: 0.0,
            top: -2.0,
            oneshot: None,
            overlay: None,
            base_ticks: 0.0,
            event_scale: None,
            turn_pct: 0,
            look_pct: 0,
            last_attack: String::new(),
            extra: Vec::new(),
            fx_files: Vec::new(),
            wep_hide: [[false; 2]; 16],
            wep_show_at: [None; 16],
            play: None,
            pending: None,
            battle: Default::default(),
            blend_len: crate::motion::BLEND_TICKS,
            alpha: actmotion::OPAQUE,
            rgb: [actmotion::OPAQUE; 3],
            flash: Default::default(),
            fade: None,
            spop: false,
            bones: Vec::new(),
            scale: 1.0,
            top_key: String::new(),
            find_cache: Default::default(),
            failed: false,
            motion_names: None,
            rate_scale: 1.0,
            ride_on: Vec::new(),
            appear: 0.0,
            mouth_name: "mou4",
            ground_tilt: Default::default(),
            tilt_rot: None,
            mouth: None,
            blink_wait: (actmotion::crand() % 480) as f32,
            blink: None,
            look: Default::default(),
            emote: None,
            rest: Vec::new(),
            jump: -1.0,
            pick_bones: None,
        }
    }

    /// Faded out completely (a 0x038 "kesu" or op 0x6c): not drawn.
    fn vanished(&self) -> bool {
        self.alpha <= 0.0
    }

    /// Posed with real size: helper NPCs use stub models (50, 52) that are a few millimetres tall.
    fn shown(&self) -> bool {
        self.gpu.is_some() && self.top < -0.1 && !self.vanished()
    }
}

/// DAT_10480aec (FUN_1008db40 asm:151886), with /battlebgm off (config 0x99 bit 0 clear): a monster with a model,
/// not a door / lift / ship, GM level under 3, no team colour, claimed by anyone on our alliance list (us included),
/// not owned (a charmed monster counts as owned *(inf: no pet index for others)*), within 45 of us; not while we are
/// KO'd or in an event *(inf: retail tests the field state 0x60)*.
fn party_battle(w: &World) -> bool {
    if w.game.get(0x99) & 1 != 0 || matches!(w.my_motion.status, 2 | 3) || event::active(w) {
        return false;
    }
    w.entities.iter().any(|(id, e)| {
        e.flags & 1 != 0
            && !matches!(e.sub_kind, 2..=4)
            && e.marks.gm < 3
            && !((2..=99).contains(&e.marks.alleg) && !(0x28..=0x2b).contains(&e.marks.alleg))
            && !e.marks.charm
            && e.claim != 0
            && (e.claim == w.my_id || w.party.members().any(|m| m.id == e.claim))
            && w.actors.get(id).is_some_and(|a| a.character.is_some())
            && (e.pos - w.pos).length() < 45.0
    })
}

/// FUN_100cd040 with R 0.5 / r 0.2 (the 0x5e-chunk radii of a few large models are not kept): the eye's horizontal
/// distance h from point `p`; 0 within 0.2, linear to 1 at 0.5; no fade with the eye more than 0.5 above the point.
fn near_camera_fade(p: Vec3, eye: Vec3) -> f32 {
    let dy = if p.y < eye.y { 0.0 } else { p.y - eye.y };
    let h = Vec3::new(p.x - eye.x, 0.0, p.z - eye.z).length();
    if h >= 0.5 || dy > 0.5 {
        1.0
    } else if h < 0.2 {
        0.0
    } else {
        (h - 0.2) / 0.3
    }
}

/// Zone NPC names (FUN_10093bd0): file 0x1a40 + zone (0x150db + zone from 256), 0x20-byte records of
/// name (24 bytes; the client clears +0x18) and u32 server id at +0x1c.
fn npc_names(index: &DatIndex, zone: u16) -> HashMap<u32, String> {
    let id = if zone < 0x100 { 0x1a40 + zone as u32 } else { 0x150db + zone as u32 };
    let Ok(d) = index.read(id) else { return HashMap::new() };
    d.chunks_exact(0x20)
        .filter_map(|r| {
            let id = u32::from_le_bytes(r[0x1c..0x20].try_into().ok()?);
            let name = &r[..r[..0x18].iter().position(|&b| b == 0).unwrap_or(0x18)];
            (id != 0 && !name.is_empty()).then(|| (id, String::from_utf8_lossy(name).into_owned()))
        })
        .collect()
}

/// Lobby/zone GrapIDTbl: [0] is face | race << 8, the rest carry the slot in the top nibble.
fn look_from(tbl: [u16; 9]) -> Look {
    let id = |i: usize| tbl[i] & 0x0FFF;
    Look { race: (tbl[0] >> 8) as u8, face: tbl[0] & 0xFF, head: id(1), body: id(2), hands: id(3), legs: id(4), feet: id(5), main: id(6), sub: id(7), range: id(8) }
}

const TAU_4096: f32 = std::f32::consts::TAU / 4096.0;

/// Where and how an actor is drawn this frame: position, facing, status, motion ratio, size.
#[derive(Clone, Copy)]
struct Place {
    pos: Vec3,
    forward: Vec3,
    /// Op 0xd8's rotation x / z on an event body.
    tilt: [f32; 2],
    status: u8,
    ratio: f32,
    size: u8,
}

/// The application icon (assets/vana-client.png at 256 px) for the window and the task bar.
fn window_icon() -> Option<winit::window::Icon> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(&include_bytes!("../../assets/icon-256.png")[..]));
    dec.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    winit::window::Icon::from_rgba(buf, info.width, info.height).ok()
}

/// Where the player is drawn: between the physics steps (drawn_between_steps), or where something else just put us.
fn drawn_pos(w: &World) -> Vec3 {
    if w.pos == w.step_to { w.draw_pos } else { w.pos }
}

/// Not retail (it draws each frame's collision position): the drawn player runs through the averages of three
/// physics steps, `t` of the way from the older one. Exact on a steady slope, one step late; it cancels the
/// 3-step bob of running downhill (the ground move stays level, gravity catches up) and spreads step-ups.
fn drawn_between_steps(h: &[Vec3; 4], t: f32) -> Vec3 {
    // As offsets from the middle step, so a player standing still is drawn exactly where they stand (a plain sum / 3
    // wobbles in the last bit, and the camera and the name plate's pixel rounding with it).
    let a = h[1] + (h[0] - h[1] + (h[2] - h[1])) / 3.0;
    let b = h[2] + (h[1] - h[2] + (h[3] - h[2])) / 3.0;
    a + (b - a) * t
}

/// The event input window (FUN_10206640) over the msgline rect: kinds 1..5 a group 0xb title, the text after it;
/// others the text at +80 (kind 0 with the "Password:" child). 200 px of text around the caret, a steady '|'.
fn draw_event_input(gu: &mut GameUi, kind: i32, text: &str, caret: usize, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let hide: &[u16] = if kind == 0 { &[] } else { &[1] };
    let win = gu.lobby_window_off("menu    passinpu", sw, sh, (0.0, 0.0), None, false, hide)?;
    let [x, y, ..] = win.frame;
    let fx = match kind {
        1..=5 => {
            let title = gu.group_b([0xb9, 0xbe, 0xb9, 0xc8, 0xc9][kind as usize - 1]).unwrap_or_default();
            gu.text(&title, x + 4.0, y + 4.0, 0, WHITE);
            x + 4.0 + gu.text_width(&title)
        }
        _ => x + 80.0,
    };
    let caret = caret.min(text.len());
    let before = fit_tail(gu, &text[..caret], 200.0);
    let room = 200.0 - gu.text_width(&before);
    let mut used = 0.0;
    let after: String = text[caret..]
        .chars()
        .take_while(|c| {
            used += gu.text_width(&c.to_string());
            used <= room
        })
        .collect();
    let cx = fx + gu.text_width(&before);
    gu.text(&format!("{before}{after}"), fx, y + 4.0, 0, WHITE);
    gu.text("|", cx, y + 4.0, 0, WHITE);
    Some(win.frame)
}

/// The item info window (FUN_1013cba0 / FUN_1013d760) for the open list's cursor item, else the event's: mode 0 at
/// (16, 240) stacked on the log (frame top = log top - height - 2, at least 16), mode 1 at (130, 240).
fn draw_item_info(gu: &mut GameUi, world: &World, sw: f32, sh: f32, log_top: f32) -> Option<[f32; 4]> {
    let cache = world.item_info.clone()?;
    let post_item = post::info_source(world).or_else(|| synth::info_source(world));
    let (id, ext, container, equipped, mode, flags) = match (items::info_source(world), event::option_item(world)) {
        (Some((id, ext, c, _, eq, mode)), _) => (id, ext, Some(c), eq, mode, 0),
        // The shop list's row: mode 1 at (130, 240), right of the money window (FUN_1013e3b0(1, ..)).
        (None, _) if let Some((id, ext)) = shop::info_source(world) => (id, ext, None, false, 1, 0),
        (None, _) if let Some((id, ext)) = trade::info_source(world) => (id, ext, None, false, 0, 0),
        (None, _) if let Some((id, ext)) = inspect::info_source(world) => (id, ext, None, false, 0, 0),
        (None, _) if let Some((id, ext)) = esedit::info_source(world) => (id, ext, None, false, 0, 0),
        (None, _) if let Some((id, ext)) = post_item => (id, ext, None, false, 0, 0),
        (None, _) if let Some((id, ext)) = treasure::info_source(world) => (id, ext, None, false, 0, 0),
        (None, Some((id, ext, f))) => (id, ext, None, false, 4, f as u32),
        _ => world.info_win.map(|(id, ext, f)| (id, ext, None, false, 0, f))?,
    };
    let it = cache.item(id)?;
    let mut p = world.info_page.get();
    if p.1 != id {
        p = (0, id, 1);
    }
    let cx = iteminfo::Ctx { now: vana_now(world) as u32, equipped, shop: false, container, flags };
    let aug = gu.aug_ctx();
    let info = iteminfo::build(&it, ext.as_ref(), &cx, &|g, k| gu.item_msg(g, k), &|s| gu.text_width(s), aug.as_ref());
    p.2 = info.pages.len();
    world.info_page.set(p);
    let page = p.0.min(info.pages.len().saturating_sub(1));
    // itemxinf open: both windows take the larger row count (+0x381 / +0x382); the compare shows its first page (inf).
    let cmp = items::compare(world);
    let cmp_info = cmp.flatten().and_then(|(cid, cext, c)| {
        let cit = cache.item(cid)?;
        let cx = iteminfo::Ctx { now: vana_now(world) as u32, equipped: true, shop: false, container: Some(c), flags: 0 };
        Some((cid, iteminfo::build(&cit, cext.as_ref(), &cx, &|g, k| gu.item_msg(g, k), &|s| gu.text_width(s), aug.as_ref())))
    });
    let rows = iteminfo::rows(&info, page).max(cmp_info.as_ref().map_or(3, |(_, ci)| iteminfo::rows(ci, 0)));
    let layout = iteminfo::LAYOUTS[rows - 3];
    let [fx, fy, _, fh] = gu.window_rect(layout, sw, sh)?;
    // Mode 4 (FUN_1013e4b0): (16, 16) stacked on region 3, taken as the choice window *(inf)*.
    let query_top = gu.window_rect("menu    query", sw, sh).map_or(16.0 + fh + 2.0, |r| r[1]);
    let off = match mode {
        // FUN_1013e590 case 1: (130, 240) from the top left, whatever the layout's anchor (item4inf..item6inf sit at
        // the bottom).
        1 => (130.0 - fx, 240.0 - fy),
        4 => (0.0, (query_top - fh - 2.0).max(16.0) - fy),
        _ => (0.0, (log_top - fh - 2.0).max(16.0) - fy),
    };
    let r = gu.window_swapped("menu    iteminfo", layout, sw, sh, off)?;
    items::upload_icons(gu, &cache, std::iter::once(id));
    let icon = crate::render::invwin::icon_texture(id);
    iteminfo::draw(gu, &info, page, gu.has_image(&icon).then_some(icon.as_str()), r[0], r[1]);
    // FUN_1013ad50: itemyinf right above iteminfo, itemxinf above that, both at its x.
    if cmp.is_some() {
        let [yx, yy, _, yh] = gu.window_rect("menu    itemyinf", sw, sh)?;
        gu.lobby_window_off("menu    itemyinf", sw, sh, (r[0] - yx, r[1] - yh - yy), None, false, &[]);
        let xl = iteminfo::XLAYOUTS[rows - 3];
        let [xx, xy, _, xh] = gu.window_rect(xl, sw, sh)?;
        let rx = gu.window_swapped("menu    itemxinf", xl, sw, sh, (r[0] - xx, r[1] - yh - xh - xy))?;
        if let Some((cid, ci)) = &cmp_info {
            items::upload_icons(gu, &cache, std::iter::once(*cid));
            let icon = crate::render::invwin::icon_texture(*cid);
            iteminfo::draw(gu, ci, 0, gu.has_image(&icon).then_some(icon.as_str()), rx[0], rx[1]);
        }
    }
    Some(r)
}

/// FUN_100c63d0: the drawn rotation eases 0.125 a tick toward the actor's, or op 0x59's percent of 0.00125 during an
/// event.
fn turn_rate(a: &Actor, in_event: bool) -> f32 {
    if in_event && a.turn_pct != 0 { (a.turn_pct as f32 * 0.00125).clamp(0.0, 1.0) } else { 0.125 }
}

/// An actor's place; a riding animal stands at its rider's (FUN_100c8ed0), a rider on a mount moves by its offset and
/// yaw (FUN_100c9330; zero on a chocobo).
fn actor_place(w: &World, id: u32, my_dir: u8) -> Option<Place> {
    // Skinned furniture stands at its piece's place (client/furniture.rs).
    if let Some((pos, yaw)) = w.furniture.place_of(id) {
        return Some(Place { pos, forward: Vec3::new(yaw.cos(), 0.0, -yaw.sin()), tilt: [0.0; 2], status: 0, ratio: 0.0, size: 0 });
    }
    if id & ride::ANIMAL != 0 {
        if let Some((pos, forward)) = ride::left_at(w, id) {
            return Some(Place { pos, forward, tilt: [0.0; 2], status: 0, ratio: 0.0, size: 0 });
        }
        let p = base_place(w, id & !ride::ANIMAL, my_dir)?;
        return Some(Place { status: 0, tilt: [0.0; 2], ..p });
    }
    let mut p = base_place(w, id, my_dir)?;
    if let Some(r) = w.rides.get(&id).filter(|_| matches!(p.status, 5 | 0x55)) {
        p.pos += r.offset;
        let a = (-p.forward.z).atan2(p.forward.x) + r.yaw;
        p.forward = Vec3::new(a.cos(), 0.0, -a.sin());
    }
    Some(p)
}

/// A bound actor draws from its VM body while the event runs, posing with its event status (+0x174, op 0xac 1);
/// otherwise ourselves from the session, others from their entity.
fn base_place(w: &World, id: u32, my_dir: u8) -> Option<Place> {
    let size = if id == w.my_id { w.my_motion.graph_size } else { w.entities.get(&id)?.motion.graph_size };
    if let Some(p) = event::pose(w, id) {
        return Some(Place { pos: p.pos, forward: Vec3::new(p.yaw.cos(), 0.0, -p.yaw.sin()), tilt: p.tilt, status: event::event_status(w, id), ratio: p.ratio, size });
    }
    if id == w.my_id {
        let m = w.my_motion;
        return Some(Place { pos: drawn_pos(w), forward: forward_from_dir(my_dir), tilt: [0.0; 2], status: m.status, ratio: if w.moving { w.move_tier } else { 0.0 }, size });
    }
    let e = w.entities.get(&id)?;
    Some(Place { pos: e.pos, forward: Vec3::new(e.yaw.cos(), 0.0, -e.yaw.sin()), tilt: [0.0; 2], status: e.motion.status, ratio: e.ratio, size })
}

/// The drawn frame without the scale: the ground tilt's rotation at the position, else the plain one.
fn actor_frame(p: &Place, a: &Actor) -> glam::Mat4 {
    match a.tilt_rot {
        Some(r) => glam::Mat4::from_translation(p.pos) * glam::Mat4::from_mat3(r),
        None => plain_frame(p.pos, p.forward, p.tilt),
    }
}

/// An actor's attach point `k` in the world from its rest pose (vtable +0x1c4); without one its position lifted
/// by 1.2 x its height (FUN_10086820's fallback).
fn attach_point(w: &World, places: &HashMap<u32, Place>, id: u32, k: u16) -> Option<Vec3> {
    let p = places.get(&id)?;
    let a = w.actors.get(&id)?;
    let ch = a.character.as_ref()?;
    let model = actor_frame(p, a) * glam::Mat4::from_scale(Vec3::splat(a.scale));
    match ch.skeleton.points.get(k as usize).and_then(|(bone, off)| a.rest.get(*bone as usize).map(|(q, t)| *t + *q * *off)) {
        Some(local) => Some(model.transform_point3(local)),
        None => Some(p.pos + Vec3::new(0.0, a.top * 1.2, 0.0)),
    }
}

/// The world's actors as the effect system sees them (fx::Actors): placements, rest-pose attach points, files.
struct FxActors<'a> {
    actors: &'a HashMap<u32, Actor>,
    places: &'a HashMap<u32, Place>,
    contacts: &'a HashMap<u32, (Vec3, Vec3)>,
    entities: &'a HashMap<u32, Entity>,
    me: u32,
    me_moving: bool,
    pet: u16,
    /// Our party (roster mode 0, FUN_100e5dd0(0)): ourselves first; our ActIndex (we are not in `entities`).
    party: Vec<u32>,
    my_index: u16,
    /// The area environments' tints this frame, and the floor to find an actor's area on.
    areas: &'a HashMap<u32, Vec3>,
    ground: Option<&'a ground::Ground>,
    /// /bellsw (config 0x99 bit 1): scheduler op 0xb3 sounds.
    bells: bool,
    /// Mog House furniture: id, model file, matrix (client/furniture.rs).
    furniture: &'a [(u32, u32, glam::Mat4)],
}

impl FxActors<'_> {
    fn furniture(&self, id: u32) -> Option<&(u32, u32, glam::Mat4)> {
        self.furniture.iter().find(|f| f.0 == id)
    }
}

impl crate::fx::Actors for FxActors<'_> {
    fn bells(&self) -> bool {
        self.bells
    }

    fn matrix(&self, id: u32) -> Option<glam::Mat4> {
        if let Some(f) = self.furniture(id) {
            return Some(f.2);
        }
        let p = self.places.get(&id)?;
        Some(match self.actors.get(&id) {
            Some(a) => actor_frame(p, a),
            None => plain_frame(p.pos, p.forward, p.tilt),
        })
    }

    fn moving(&self, id: u32) -> bool {
        if id == self.me { self.me_moving } else { self.entities.get(&id).is_some_and(|e| e.ratio > 1e-4) }
    }

    fn cib(&self, id: u32) -> Option<[i8; 3]> {
        self.actors.get(&id)?.character.as_ref().map(|c| c.cib)
    }

    fn sub_anim(&self, id: u32) -> u8 {
        self.entities.get(&id).map_or(0, |e| e.sub[2])
    }

    fn ranged_class(&self, id: u32) -> Option<i8> {
        self.actors.get(&id)?.character.as_ref().map(|c| c.ranged)
    }

    fn model_gens_off(&self, id: u32) -> bool {
        self.entities.get(&id).is_some_and(|e| e.gens_off)
    }

    fn player(&self) -> Option<u32> {
        Some(self.me)
    }

    fn swings(&self, id: u32, moving: bool) -> Option<Vec<String>> {
        let a = self.actors.get(&id)?;
        let ch = a.character.as_ref()?;
        let names = actmotion::scheduler_names(&a.extra, ch);
        let sub = self.entities.get(&id).map_or(0, |e| e.sub[2]);
        let (first, other) = combat::swing_patterns(moving, sub);
        let (first, other) = (first.as_str(), other.as_str());
        let mut pool = crate::dat::sched::matching(names.iter().copied(), first);
        if pool.is_empty() {
            pool = crate::dat::sched::matching(names.iter().copied(), other);
        }
        Some(pool.into_iter().map(str::to_string).collect())
    }

    // FUN_100d4560 -> FUN_1002c400: point 2 is the root plus its offset (no bone, no pose); the others ride the drawn
    // pose's bones (posed for effect actors, see posed_actors).
    fn point(&self, id: u32, k: u16) -> Option<Vec3> {
        let a = self.actors.get(&id)?;
        let ch = a.character.as_ref()?;
        let (bone, off) = ch.skeleton.points.get(k as usize)?;
        if k == 2 {
            return Some(*off * a.scale);
        }
        let bones = if a.bones.is_empty() { &a.rest } else { &a.bones };
        bones.get(*bone as usize).map(|(q, t)| (*t + *q * *off) * a.scale)
    }

    fn point_frame(&self, id: u32, k: u16) -> Option<glam::Mat4> {
        let a = self.actors.get(&id)?;
        let ch = a.character.as_ref()?;
        let (bone, off) = *ch.skeleton.points.get(k as usize)?;
        let r = ch.skeleton.point_rots.get(k as usize).copied().unwrap_or(Vec3::ZERO);
        let bones = if a.bones.is_empty() { &a.rest } else { &a.bones };
        let (q, t) = *bones.get(bone as usize)?;
        let local = glam::Mat4::from_translation(off) * glam::Mat4::from_rotation_z(r.z) * glam::Mat4::from_rotation_y(r.y) * glam::Mat4::from_rotation_x(r.x);
        let mut f = glam::Mat4::from_rotation_translation(q, t) * local;
        f.w_axis = (f.w_axis.truncate() * a.scale).extend(1.0);
        Some(f)
    }

    fn status(&self, id: u32) -> u8 {
        self.places.get(&id).map_or(0, |p| p.status)
    }

    fn decal_cell(&self, c: Vec3) -> Option<(usize, usize)> {
        self.ground?.decal_cell(c)
    }

    fn decal_tris(&self, c: Vec3) -> Vec<Vec3> {
        self.ground.map_or(Vec::new(), |g| g.decal_tris(c))
    }

    fn ground_hit(&self, from: Vec3, to: Vec3) -> Option<Vec3> {
        let d = self.ground?.raycast(from, to)?;
        Some(from + (to - from).normalize_or_zero() * d)
    }

    fn scale(&self, id: u32) -> f32 {
        self.actors.get(&id).map_or(1.0, |a| a.scale)
    }

    fn files(&self, id: u32) -> (Vec<u32>, Vec<u32>) {
        if let Some(f) = self.furniture(id) {
            return (Vec::new(), vec![f.1]);
        }
        let Some(a) = self.actors.get(&id) else { return (Vec::new(), Vec::new()) };
        let Some(ch) = a.character.as_ref() else { return (Vec::new(), Vec::new()) };
        (ch.part_files.clone(), a.extra.iter().rev().map(|(_, f)| f.id).chain(ch.files.iter().copied()).collect())
    }

    /// Actor +0xb2 bit 1 (the server's hide bits), not alpha: a 'spop' actor at alpha 0 still takes generators.
    fn hidden(&self, id: u32) -> bool {
        self.furniture(id).is_none() && (!self.actors.contains_key(&id) || self.entities.get(&id).is_some_and(|e| e.flags & 2 != 0))
    }

    fn models(&self) -> Vec<(u32, Vec<u32>)> {
        let actors = self.actors.iter().filter(|(id, _)| self.places.contains_key(id)).filter_map(|(id, a)| a.character.as_ref().map(|ch| (*id, ch.files.clone())));
        actors.chain(self.furniture.iter().map(|f| (f.0, vec![f.1]))).collect()
    }

    fn pc(&self, id: u32) -> Option<(u8, i8)> {
        let pc = self.actors.get(&id)?.character.as_ref()?.pc?;
        Some((pc.race, pc.skirt))
    }

    fn foot(&self, id: u32) -> Option<Vec3> {
        self.contacts.get(&id).map(|c| c.0)
    }

    fn floor(&self, id: u32) -> Option<Vec3> {
        self.contacts.get(&id).map(|c| c.1)
    }

    // FUN_100ac560: you or your pet, a monster (+0x120 bit 13 = u8 @0x20 bit 0), else not in the party roster
    // *(the roster is not tracked (s2c 0x0DD), so it holds only you; party members count as others)*.
    // FUN_101819a0(actor+0x5e4): the area of the floor under it *(looked up when asked, not every 120 frames; the
    // event op 0xae sub 7 override is not kept)*.
    fn area_light(&self, id: u32) -> Option<Vec3> {
        let p = self.places.get(&id)?;
        let area = self.ground?.area(p.pos)?;
        self.areas.get(&area).copied()
    }

    fn fx_class(&self, id: u32) -> u8 {
        let e = self.entities.get(&id);
        let mine = id == self.me || (self.pet != 0 && e.is_some_and(|e| e.index == self.pet));
        // FUN_1008ca20 / FUN_1008cb10: a dynamic entity's master, its +0x1cc bit 2 (in our party, FUN_1008cb80).
        let master = e.filter(|e| e.index > 0x6ff && e.master != 0).map(|e| e.master);
        let master_listed = master.is_some_and(|m| m == self.my_index || self.entities.iter().any(|(mid, me)| me.index == m && self.party.contains(mid)));
        crate::fx::filter_class(mine, e.is_some_and(|e| e.flags & 1 != 0), self.party.contains(&id), master_listed)
    }
}

/// Loaded looks kept while no actor wears them.
const MODEL_CACHE: usize = 96;

/// Model space faces +x; place it at `pos` facing `forward` (xz).
/// FUN_100c63d0 / FUN_100cbde0's cull of one box (model space corners through `model`): None when every corner is
/// behind the near plane (view z > -0.1, right-handed) or its screen rect misses the viewport; else the NDC rect and
/// the occlusion test's depth, a quarter of the way from the box's nearest to its farthest corner (0.01 nearest when
/// a corner had to be pulled onto the near plane).
fn box_on_screen(view: glam::Mat4, proj: glam::Mat4, model: glam::Mat4, corners: &[Vec3; 8]) -> Option<([f32; 4], f32)> {
    let vs: Vec<Vec3> = corners.iter().map(|c| view.transform_point3(model.transform_point3(*c))).collect();
    if vs.iter().all(|v| v.z > -0.1) {
        return None;
    }
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let mut clamped = false;
    for v in &vs {
        clamped |= v.z > -0.1;
        let v = Vec3::new(v.x, v.y, v.z.min(-0.1));
        let c = proj * v.extend(1.0);
        let n = c.truncate() / c.w;
        (lo, hi) = (lo.min(n), hi.max(n));
    }
    if clamped {
        lo.z = 0.01;
    }
    (hi.x >= -1.0 && lo.x <= 1.0 && hi.y >= -1.0 && lo.y <= 1.0).then(|| ([lo.x, lo.y, hi.x, hi.y], lo.z + 0.25 * (hi.z - lo.z)))
}

/// A skeleton box's corners in FUN_100c63d0's order (b0 / b1 and b2 / b3 one apart when equal).
fn box_corners(b: [f32; 6]) -> [Vec3; 8] {
    let [mut b0, mut b1, mut b2, mut b3, b4, b5] = b;
    if b0 == b1 {
        (b0, b1) = (b0 - 1.0, b1 + 1.0);
    }
    if b2 == b3 {
        (b2, b3) = (b2 - 1.0, b3 + 1.0);
    }
    [
        Vec3::new(b2, b1, b5), Vec3::new(b3, b1, b5), Vec3::new(-b2, b1, b4), Vec3::new(-b3, b1, b4),
        Vec3::new(-b2, b0, b5), Vec3::new(-b3, b0, b5), Vec3::new(b2, b0, b4), Vec3::new(b3, b0, b4),
    ]
}

/// The box for a status (FUN_100cfad0): 2 dead, 1 engaged / sitting / riding, else 0.
fn box_index(status: u8) -> usize {
    match status {
        2 | 3 => 2,
        1 | 5 | 6 | 0x26..=0x2b | 0x32..=0x35 | 0x38..=0x3e | 0x55 => 1,
        _ => 0,
    }
}

/// FUN_100cbde0's plain frame: T . RotY(yaw); with op 0xd8's x / z (FUN_100843b0) D3DX Scale . RotZ(z) . RotY(yaw)
/// . RotX(x), i.e. z about the model's Z and x about the world X after the yaw, both positive
/// (ground_tilt_spec.md 6).
fn plain_frame(pos: Vec3, forward: Vec3, t: [f32; 2]) -> glam::Mat4 {
    let m = actor_matrix(pos, forward);
    if t == [0.0; 2] {
        return m;
    }
    glam::Mat4::from_translation(pos) * glam::Mat4::from_rotation_x(t[0]) * glam::Mat4::from_translation(-pos) * m * glam::Mat4::from_rotation_z(t[1])
}

fn actor_matrix(pos: Vec3, forward: Vec3) -> glam::Mat4 {
    let f = Vec3::new(forward.x, 0.0, forward.z).normalize_or(Vec3::X);
    glam::Mat4::from_cols(f.extend(0.0), Vec3::Y.extend(0.0), f.cross(Vec3::Y).extend(0.0), pos.extend(1.0))
}

/// A zone's effect library, and DAT 0's when it was not loaded yet (fx::lib).
type ZoneEffects = (Option<crate::fx::lib::Library>, Option<crate::fx::lib::Library>);

/// Event ops 0x34 / 0x35 (FUN_100b3df0 / FUN_100b3ed0 -> FUN_10184380): another area's scene, collision, effects
/// and environment in place of the zone's while an event runs (the level-5 crystal scene shows area 541, then 0x35
/// with the zone's own id brings it back).
#[derive(Default)]
struct AreaSwitch {
    /// The area asked for, a load in flight, and the op done (its wait ends).
    want: Option<u16>,
    loading: bool,
    done: bool,
    /// The area shown, None = the zone's own.
    shown: Option<u16>,
    /// The zone's own renderer, collision, weather keys and effect library while another area is shown.
    home: Option<(Option<ZoneRenderer>, Option<ground::Ground>, Option<crate::dat::weather::ZoneWeather>, Option<usize>)>,
}

struct World {
    net: NetHandle,
    /// The zone number (NPC names, events, dialogue) and the scene drawn: its map number (the Mog House's in one,
    /// LAB_100facf0), and the scene loading.
    zone: Option<u16>,
    scene: Option<u16>,
    loading: Option<u16>,
    loading_scene: u16,
    /// Door states by entity (client/doors.rs).
    doors: doors::Doors,
    /// In the Mog House (0x00A LoginState 1), its door menu, and a leave request sent.
    myroom: Option<net::MyRoom>,
    /// The party / alliance roster (client/party.rs) and whether its names were asked for (c2s 0x076).
    party: party::Roster,
    party_asked: bool,
    /// Our 0x037 flags word (u32 @0x28): bit 4 seeking a party, bit 5 anonymous (the /anon / /invite toggles).
    self_flags: u32,
    /// The gauges' eased values (FUN_101cad00, a 30 Hz step): per party row (member id, HP, MP), the target's (id,
    /// HP), and the unstepped time.
    party_gauges: Vec<(u32, u8, u8)>,
    target_gauge: (u32, u8),
    gauge_acc: f32,
    gauge_last: Option<Instant>,
    /// The macro files and runner (client/macros.rs), loaded at each zone-in.
    macros: Option<macros::Macros>,
    /// The macro palette controller (DAT_1066279c): +0xc open (0, 1 Ctrl, 2 Alt), +0xd pending, +0x10 the hold's
    /// second, each palette's cursor cell.
    palette: (u8, u8, u64, [usize; 2]),
    /// Key function 0x56 (Scroll Lock): the UI hidden (menu manager +0xb4); shown again by Esc and on zoning.
    ui_hidden: bool,
    /// Key function 0x55 (Print Screen, UI hidden only): the screenshot to save this frame.
    shot_request: Option<std::path::PathBuf>,
    /// FUN_101d0ea0's same-second counter (the name's last letter, 'a' + n % 10) and the second it counts.
    shot_count: (u8, i8),
    /// Zone flag 0x100 (0x00A LoginState 1, 4 or 5): no grid position by the clock.
    room_flag: bool,
    mogdoor: Option<myroom::Menu>,
    leaving_room: bool,
    area: AreaSwitch,
    /// The building interior loaded over the zone.
    sub: subarea::SubArea,
    /// Event op 0xd9: effect sounds get 20 voices while an event runs (FUN_10038330).
    event_voices: bool,
    /// Looks set by event op 0xb6 (and the riding animals, ride.rs), for the model loader.
    event_looks: Vec<(u32, Source)>,
    /// Our riding fields (0x037, 0x067), and every rider's state (ride.rs).
    my_ride: net::Ride,
    rides: HashMap<u32, ride::Riding>,
    /// Event music fades (ops 0x5d, 0x5c 0xa0 / 0xa1): volume, ticks.
    music_fades: Vec<(i32, i32)>,
    /// The audio still changing music (event op 0x9a waits), as of the last frame.
    music_changing: bool,
    renderer: Option<ZoneRenderer>,
    /// Effect generators of the zone (fx/).
    effects: crate::fx::Effects,
    /// Per actor: its foot points' last heights relative to the root (actor+0x7e8..+0x7f4).
    feet: HashMap<u32, [f32; 4]>,
    /// Per actor: the last step's foot contact in the world (actor+0x63c) and the floor normal there.
    contacts: HashMap<u32, (Vec3, Vec3)>,
    /// Our pet's ActIndex (0x037 u32 @0x34 >> 3; 0 = none).
    my_pet: u16,
    /// Area environment tints (area 4cc -> Env::area_light) at the current time and weather.
    area_lights: HashMap<u32, Vec3>,
    ground: Option<ground::Ground>,
    name: String,
    my_id: u32,
    /// Our ActIndex (0x00A @8), for 0x016 about ourselves.
    my_index: u16,
    pos: Vec3,
    yaw: f32,
    entities: HashMap<u32, Entity>,
    /// Actors drawn on screen this frame (actor +0xb2 bit 0 clear, FUN_100c63d0 / FUN_100cbde0): the target keys
    /// only take these.
    visible: HashSet<u32>,
    /// Zone lines with a destination (tag + destination tag).
    zone_lines: Vec<zone::Region>,
    /// The zone's lift shafts and lift actors; whether our last step stood on one (+0x5e2), and our y then (the
    /// camera follows the lift, DAT_10456d9c).
    lifts: lifts::Lifts,
    /// The character we are held against (client/bump.rs).
    bump: bump::State,
    riding: bool,
    ride_y: f32,
    /// Since when the zone-in fade waits for a lift actor.
    lift_wait: Option<Instant>,
    zoning_since: Option<Instant>,
    chat: combat::ChatLog,
    chat_input: String,
    /// The chat line's caret (char index; None the end of the line, where every new line starts).
    chat_caret: Option<usize>,
    /// The other end of the chat line's selection (Shift + arrows; ours), and text for the clipboard this frame.
    chat_anchor: Option<usize>,
    copied: Option<String>,
    /// The auto-translate candidate list (menu fep) while open.
    fep: Option<fep::Fep>,
    /// The log window's size and scroll state, and the top bar / Network / compass state.
    log: logwin::LogWin,
    hud_top: hudtop::HudTop,
    /// Who sent us tells, most recent first (Ctrl+R).
    tells: Vec<String>,
    /// Battle message table, names and colours (loaded in the background).
    battle: Option<Arc<crate::dat::battle::BattleText>>,
    /// Melee results waiting for the attacker's hit.
    pending: Vec<combat::Pending>,
    /// Swing holds (scheduler ops 0x20 / 0x59): until when an actor's server status changes wait (the applied status
    /// +0x170 vs the raw +0x16c), the waiting status, and until when it plays no hit reactions (op 0x59).
    holds: HashMap<u32, Instant>,
    /// The rest of a melee round by attacker: when the next swing starts, and its records (combat::on_action).
    rounds: HashMap<u32, (Instant, std::collections::VecDeque<crate::net::action::Action>)>,
    held_status: HashMap<u32, u8>,
    noreact: HashMap<u32, Instant>,
    /// Scheduler ops 0x2e (our walk input) and 0x2f (an actor's facing) locked until then.
    move_lock: Option<Instant>,
    face_locks: HashMap<u32, Instant>,
    /// Skill actions' load holds (busy counts, FUN_100a4140) with their safety deadline.
    loads: HashMap<u32, (u32, Instant)>,
    /// Running knockbacks (op 0x5e), by actor.
    knocks: HashMap<u32, combat::KnockState>,
    /// The action menu (Enter), and when the world started (a seed for random motion picks).
    action_menu: Option<combat::ActionMenu>,
    /// A main menu row was picked this frame (the snapshot's `picked`).
    menu_picked: bool,
    /// The window manager's last applied status (FUN_1015ef60 +0x68): its edges into / out of 1 run the battle reset.
    battle_prev: u8,
    /// "menu    gaugewin", up while engaged.
    gauge: Option<gauge::Gauge>,
    /// Our status icons (0x037, 0xffff none) and the last 0x063 sub 9 timers (kept once received, as retail).
    buffs: [u16; 32],
    buff_timers: Option<([u16; 32], [u32; 32])>,
    /// The buff list's cursor (Num+ focus step 2).
    buff_cursor: usize,
    /// Our PvP team byte (+0x1ff): Conflict rows by FUN_101cc350.
    my_team: u8,
    /// Our plate's fields from 0x037 and our 0x067 @0x10.
    my_plate: net::SelfPlate,
    my_sync: u8,
    /// FUN_101347b0's repeat guard: the last check's id and time (DAT_105781c4).
    last_check: Option<(u32, Instant)>,
    /// /blockhelp (DAT_10578175): Right on Disengage refuses Call for help.
    block_help: bool,
    /// Party members' status icons by UniqueNo (s2c 0x076).
    party_buffs: HashMap<u32, [u16; 32]>,
    /// The action menu suspended under the list it opened (Magic, Abilities, Items, Trust: their groups share playermo's
    /// 0x04): the walk brings it back with its cursor when the list is gone.
    held_menu: Option<combat::ActionMenu>,
    /// FUN_10118830 / FUN_10118870: windows' cursor children kept in their layouts across closes, and the main menu's
    /// row per page.
    cursor_mem: HashMap<&'static str, u16>,
    menu_mem: [usize; 2],
    started: Instant,
    logging_out: bool,
    target: Option<u32>,
    /// Target lock-on (DAT_10565d34 +0x5c bits 0..1 != 0), the locked camera, ticks left in which the free
    /// camera ignores manual turns after a lock (DAT_10444464), auto lock-on (config 7 = 0), when the target
    /// was set, and manual camera turns waiting for the locked camera.
    locked: bool,
    cam: target::LockCam,
    /// The locked camera's target height, taken once at lock-on (actor +0x8a8's point).
    lock_height: f32,
    unlock_ticks: f32,
    /// The free camera's state (camera.rs), and our status last frame.
    camst: camera::State,
    last_status: u8,
    auto_lock: bool,
    /// Loaded models by source key, shared by every actor with that look, and keys being loaded.
    models: HashMap<String, Arc<Model>>,
    models_loading: HashSet<String>,
    /// A new zone renderer still uploading (since when).
    zone_uploading: Option<Instant>,
    /// Config `party_tp` (retail config 0xae bit 0); /partyinfo asks the App to save it.
    party_tp: bool,
    party_tp_save: bool,
    /// /itemsearch's c2s 0x02C is waiting for s2c 0x049 (session +0x3c148).
    item_search: bool,
    /// The last proposer (s2c 0x078 sName, DAT_104df910): /vote's default.
    last_proposer: String,
    /// /translate's c2s 0x02B is waiting for s2c 0x047 (G+0x3c140).
    translating: bool,
    /// /follow's target (0x10487f74): autorun steers at it each tick.
    follow: Option<u32>,
    target_since: Instant,
    cam_turn: (f32, f32),
    /// UI sound effects to play ("0002", "0009" ...), by SE id.
    sounds: Vec<u32>,
    /// The zone-in "=== Area: ... ===" line, waiting for the name tables (FUN_100f8730 / FUN_100f87c0).
    area_line: Option<net::AreaLine>,
    /// The zone our position belongs to, and whether it still has to be put on the ground.
    pos_zone: u16,
    unsettled: bool,
    actors: HashMap<u32, Actor>,
    moving: bool,
    /// Walk mode (0x45, DAT_10565d34 +0x50 / player +0xfa), autorun (0x1c, DAT_10475579) and its world-space
    /// vector (DAT_1047555c), and this frame's speed fraction (FUN_100a5f60: 1, 1/3 or 0).
    walk: bool,
    autorun: bool,
    autorun_dir: Vec3,
    move_tier: f32,
    /// Player physics runs in retail frames (ground::STEP_TICKS): ticks not yet stepped, the last four step
    /// results (oldest first), the position after the last step, and where the player is drawn (drawn_pos).
    step_ticks: f32,
    step_hist: [Vec3; 4],
    step_to: Vec3,
    draw_pos: Vec3,
    /// Screen fade: 0 clear, 1 black. Zoning fades out, loads, then fades back in.
    fade: f32,
    my_motion: net::MotionState,
    /// HP, MP, TP and their percentages, once the server has sent them (0x0DF).
    vitals: Option<(u32, u32, u32, u8, u8)>,
    /// While dead (FUN_10096c90's state): the deadline in game seconds (0x40ddc - 360), the K.O. window ("menu
    /// dead") open, "go home" chosen or expired (handler +0x24), and when 0x01A 0x0B was last sent (every 10 s).
    ko_deadline: u64,
    ko_open: bool,
    homing: bool,
    home_sent: Option<Instant>,
    /// FUN_10096c90's modes 2 / 3: a raise (2, "menu    resynus", Accept highlighted when `raise_yes`) or tractor (3)
    /// offer waiting for an answer; after accepting a raise, nothing until (sub 4, 1200 ticks).
    ko_offer: u8,
    raise_yes: bool,
    /// The prompt row under the pointer last frame (the hover moves the cursor only when it changes).
    raise_hover: Option<u16>,
    /// The pointer's shift while the log moves a stacked window under it (UI px, y).
    ptr_dy: f32,
    ko_wait: Option<Instant>,
    /// Server clock (GameTime and when it arrived), weather number, and the zone's weather keys.
    clock: Option<(u32, Instant)>,
    /// The compass clock (DAT_10666e84 +0x2e): its time, day orb and weather badges; /clock toggles it, on at each world
    /// set-up.
    compass_clock: bool,
    /// /names (DAT_1047d694 / DAT_1047d695): hidden; true hides only other players' names.
    names_hidden: Option<bool>,
    /// /mutebgm, /mutese (DAT_103517f8 / DAT_103516fc), session only.
    bgm_mute: bool,
    se_mute: bool,
    /// s2c 0x00A PlayTime (u32 @0xA0) and when it came, for /playtime.
    play_time: Option<(u32, Instant)>,
    /// The chat mode's prefix for a line without a command (/chatmode, chat-line object +8).
    chat_prefix: String,
    /// DAT_10662da4: the chat mode (+0x108: 2 say, 3 shout, 4 tell, 5 party, 6 ls, 0xb unity, 0xc ls2 ...), the previous
    /// mode and prefix (+0x10c / +0x88), the current tell name (DAT_1066268c +0x10e) and the sent-tell names (+8, 8 at
    /// most, newest first; retail keeps them in acq.dat, ours for the session).
    chat_mode: u8,
    chat_prev: (u8, String),
    tell_name: Option<String>,
    tell_hist: Vec<String>,
    /// The chat mode window ("menu    chatctrl").
    chatctrl: Option<chatctrl::ChatCtrl>,
    /// The quest / mission store (s2c 0x056), the log name files (read on the first open), the Missions windows, a
    /// /mission to open (with its argument), and the enabled expansions (lobby bits & installed, FUN_1022dad0).
    journal: journal::Store,
    /// Our adventuring fellow's ActIndex (0x037 u16 @0x48, player +0x29c).
    my_fellow: u16,
    /// The Search row's windows (searchma and its criteria), and scname's name (kept between opens, +0x18).
    searchmenu: Option<searchmenu::SearchMenu>,
    search_name: String,
    /// W+0x5c (the last search's condition, which scsibori refines) and DAT_106644d8 (the first status searched since
    /// searchma opened or /sea ran).
    search_cond: crate::net::search::Cond,
    search_status0: u32,
    /// The area picker and its view (+0x5c: by region; not saved).
    arealist: Option<arealist::AreaList>,
    arealist_by_region: bool,
    /// Region Info: the conquest data and marker tables, the region menu, the map (cnqframe), a /regionmap to open.
    conquest: region::Data,
    region_menu: Option<region::Menu>,
    cnq: Option<region::CnqMap>,
    region_cmd: Option<usize>,
    /// The own search comment (shared with /seacom) and its editor while open.
    seacom: seacom::Store,
    seacom_edit: Option<seacom::Editor>,
    /// The Linkshell row's windows, the change sent last, and which of link2 / link3 / link4 have opened once.
    linkshell: Option<linkshell::Linkshell>,
    ls_pending: Option<linkshell::Pending>,
    ls_opened: u8,
    /// The linkshell message's ask states and reasons, and S+0x9852 (the level request's sequence).
    ls_msg: linkshell::MsgState,
    ls_msg_seq: u16,
    /// The Key Items windows and the key items' seen bits (s2c 0x055 @0x44).
    keyitems: Option<keyitems::KeyItems>,
    ki_seen: keyitems::Seen,
    /// The Quests windows and the Records of Eminence (s2c 0x110..0x112).
    quests: Option<quests::Quests>,
    roe: quests::Roe,
    logs: Option<Arc<crate::dat::journal::Logs>>,
    missions: Option<missions::Missions>,
    mission_cmd: Option<Option<String>>,
    expansions: u32,
    /// The Status row's windows (client/status.rs); s2c 0x063 type 2 (limit points, bits, max merits), type 5 (job point
    /// access), types 3 / 4 (Monstrosity: the types seen as bits, type 3's rank word and prestige); 0x067 @0x25.
    status: Option<status::StatusUi>,
    merits: Option<(u16, u16, u8)>,
    jp_access: Option<u8>,
    /// Merit Points (client/merits.rs): the windows, the tables, s2c 0x08C by merit index (next cost, level) with its
    /// change stamp, the category list's remembered row (+0x54, for the session).
    merits_ui: Option<merits::MeritUi>,
    /// Linkshell's Link List (client/linklist.rs).
    linklist: Option<linklist::LinkList>,
    /// The block list window (client/blklist.rs).
    blklist: Option<blklist::BlkList>,
    /// Mog House furniture (client/furniture.rs).
    furniture: furniture::State,
    /// The main menu's pupequip (client/equipwin.rs).
    pupequip: Option<equipwin::PupMenu>,
    merit_tables: Option<Arc<crate::dat::merits::MeritTables>>,
    merit_list: std::collections::BTreeMap<u16, (u8, u8)>,
    merit_stamp: u32,
    merit_mem: usize,
    /// Job Points (client/jobpoints.rs): the windows, the tables, s2c 0x08D (level, next cost) by entry and whether one
    /// came, 0x063 type 5's (capacity, current, spent) by job.
    jp_ui: Option<jobpoints::JpUi>,
    jp_tables: Option<Arc<crate::dat::merits::JobPointTables>>,
    /// Alter Ego Points: the window, the tables and s2c 0x08E's data.
    ae_ui: Option<aepoints::AeUi>,
    ae_tables: Option<Arc<crate::dat::merits::AeTables>>,
    ae_data: Option<aepoints::AeData>,
    jp_list: HashMap<u16, (u8, u16)>,
    jp_seen: bool,
    jp_jobs: Option<[[u16; 3]; 24]>,
    monstrosity: Option<(u8, u32, u16)>,
    real_level: Option<u8>,
    /// The Party row's windows, a pending invite (s2c 0x0DC) and the s2c 0x0B4 config bytes (SAVE_CONF @4..@0xf).
    partymenu: Option<partymenu::PartyMenu>,
    invite: Option<partymenu::Invite>,
    /// c2s 0x078's pending use of the s2c 0x0E1 answer (mode, what for), and the Level Sync window.
    group_cb: Option<(u8, partymenu::GroupThen)>,
    levelsync: Option<levelsync::LevelSync>,
    conf_sys: [u8; 12],
    /// /ver's text (retail: patch.ver; ours the version string sent to the lobby).
    client_version: String,
    /// The search server from the lobby's reply (+0x40 IP, +0x44 port) and the queries running on it.
    search_addr: (std::net::Ipv4Addr, u16),
    searches: Vec<(std::sync::mpsc::Receiver<anyhow::Result<crate::net::search::Reply>>, search::Then)>,
    /// The search results window ("menu    scresult") and scoption over it.
    search: Option<search::Results>,
    /// FUN_10104f00's word lists, regions and zone flags (FFXiMain).
    search_words: Option<Arc<crate::dat::tables::SearchWords>>,
    /// s2c 0x008 EnterZoneTbl: a bit per zone visited (S+0x3c409), for /search area names.
    known_zones: Option<[u8; 0x30]>,
    /// /check on a player: the store and "menu    inspect".
    inspect: inspect::Inspect,
    /// The chat line's history (kept on the App between logins).
    line_hist: linehist::LineHistory,
    weather: u16,
    /// A weather change still blending: the weather before it, when it began and how long it takes (0x057 @0x0a
    /// Vana minutes x 2.4 s).
    weather_from: Option<(u16, Instant, f32)>,
    zone_weather: Option<crate::dat::weather::ZoneWeather>,
    /// Typing a chat line (game keys go to the input line instead).
    typing: bool,
    /// Key function 0x78's focus cycle (FUN_10151550, DAT_10565d34 +0x68: 0 none, 1 logwindo, 2 buff), whether
    /// logwindo is the active menu, and the full log (menu fulllog) when open.
    log_focus: u8,
    log_active: bool,
    full_log: Option<logwin::FullLog>,
    /// The main menu's map window, and the key item bits by word (table * 16 + word, s2c 0x055).
    map: Option<map::MapWin>,
    /// The map overlay (extra, Ctrl / Alt + O).
    overlay: Option<map::Overlay>,
    map_opacity: f32,
    /// Extras: the map overlay and the synthesis amount.
    map_overlay: bool,
    synth_amount: bool,
    /// Config `stamp_all_lines` (extra).
    stamp_all_lines: bool,
    key_items: Vec<u32>,
    /// "Downloading data ..." (menu chdummy in mode 2, FUN_1022eca0): when it opened; up from the lobby's
    /// world entry or the server's zone change until the zone has loaded (FUN_100135c0 closes it).
    downloading: Option<Instant>,
    /// Main menu open, with the highlighted row (top to bottom) and page (0 commands, 1 social).
    menu: Option<usize>,
    menu_page: u8,
    /// A yes/no prompt (FUN_102075f0) and whether Yes is highlighted.
    confirm: Option<(Confirm, bool)>,
    /// Close the window once the server has logged us out.
    shutdown: bool,
    /// Zone music slots (0x00A / 0x05F) and the choice among them.
    music: crate::audio::ZoneMusic,
    /// Game data for event, dialogue and scheduler files.
    index: Option<Arc<DatIndex>>,
    /// The running event, one waiting for its actors, and the work that outlives events (M / G).
    event: Option<Box<event::Live>>,
    event_pending: Option<Box<event::Waiting>>,
    /// Scheduler tasks, cutscene camera and screen fades (they outlive the event), and the camera drawn last.
    screen: event::Screen,
    last_camera: Option<Camera>,
    /// After an event: ticks to the next 0x016 about ourselves, until a full 0x00D comes (FUN_100ac9c0 +0x1f8).
    /// Movement is not held for it: retail players never stand frozen after talking to an NPC.
    self_lock: Option<f32>,
    /// The map window an event opened (op 0xc8) and the markers set (op 0x8b).
    map_open: Option<u16>,
    /// Event map requests for the next HUD frame (map::apply).
    map_cmds: Vec<map::Cmd>,
    /// s2c 0x063 type 6's teleport point masks (DAT_104858c4); None from c2s 0x114 until the answer.
    point_masks: Option<[u8; 64]>,
    /// Wide Scan results and tracking (the session store, lost on zoning).
    scan: widescan::Scan,
    /// The item info window opened by event ops 0x93 / 0xcc (FUN_1013e520): item, extdata, flags (+0x374).
    info_win: Option<(u16, Option<[u8; 24]>, u32)>,
    /// Its page (+0x380), the item the page is for, and the page count last built.
    info_page: std::cell::Cell<(usize, u16, usize)>,
    /// The ops' shared extdata buffer (DAT_104886bc).
    event_ext: [u8; 24],
    event_mem: crate::event_vm::Memory,
    event_end_at: Option<Instant>,
    /// Talk sent, waiting for the server (DAT_10470520).
    talk_lock: Option<Instant>,
    /// Event volume changes for the mixer: (mask, level, ticks).
    audio_cmds: Vec<(i32, f32, f32)>,
    names: Option<Arc<crate::dat::names::Names>>,
    /// Spell / ability records, and the learned bits (s2c 0x0AA, 0x0AC).
    actions: Option<Arc<crate::dat::actions::Actions>>,
    spells_known: Vec<u8>,
    commands_known: Vec<u8>,
    /// The magic / ability lists, their category windows and the target prompt (client/magic.rs, target.rs).
    magic: Option<magic::ListWin>,
    /// The lists' remembered rows (magic +0x74 / +0x98 + 4 x type, abilities +0x70 + 4 x type): (ability, list mode,
    /// type) -> id.
    list_memory: HashMap<(bool, bool, u16), u16>,
    ability: Option<magic::ListWin>,
    cats: Option<magic::Cats>,
    prompt: Option<target::Sub>,
    party_cursor: Option<target::PartyCursor>,
    /// /focustarget (target object +0x84 / +0x88): {id, ActIndex}, valid while a member of our six slots.
    focus: Option<(u32, u16)>,
    /// Owned mounts (s2c 0x0AE; None before the first) and the mount recast's end (0x119 @0xfc).
    mount_bits: Option<u64>,
    mount_recast: Option<Instant>,
    /// The emote list: the tree (file 96), the open folder path, and the unlock masks from s2c 0x11A.
    emote_tree: Option<Arc<Vec<magic::EmoteEntry>>>,
    emote_path: Vec<u16>,
    emote_jobs: u32,
    emote_chairs: u16,
    /// Alliance rows' eased HP per slot 6..17 (id, HPP), as party_gauges.
    ally_gauges: Vec<(u32, u8)>,
    /// s2c 0x067 kind 2: the visited Mog House's owner (0 = ours / none, FUN_100f9600) and the 2F / Safe 2 flag
    /// (FUN_100f97a0).
    /// 0x00A MogZoneFlag (@0xaf bit 0): a zone with a nomad moogle (FUN_101cc070(1), session +0x40dac bit 0x40).
    mog_zone: bool,
    mh_owner: u32,
    mh_floors: bool,
    /// Our first s2c 0x067 kind 2 of this zone came (S+0x40c41, FUN_100f9620).
    mh_seen: bool,
    /// Social row View House: "menu    mogcont".
    viewhouse: Option<viewhouse::ViewHouse>,
    /// statcom2's Currencies rows: evitem modes 1 / 2.
    currencies: Option<currencies::Currencies>,
    /// statcom2's Unity row (ut_menu, ut_point, quest00 / quest01 in the Unity modes) and the weeks' tables.
    unity: Option<unity::Unity>,
    unity_tables: unity::Tables,
    /// Social row Set Bazaar, our bazaar message (comment holder slots 0 / 4) and its customers (s2c 0x108).
    bazaar: Option<bazaar::Bazaar>,
    bazaar_msg: seacom::Store,
    bazaar_customers: i32,
    /// Social row Help Desk: "menu    faqsub".
    helpdesk: Option<helpdesk::HelpDesk>,
    /// The GM call's packet counters: c2s 0x0D4's Id (DAT_104e1b5c) and 0x0D3's pktId (DAT_104e1b64), never 0.
    faq_counter: u16,
    /// s2c 0x0B6's GM reply (msgId, last seqId, text, complete): the next Help Desk call shows it (client/faqmain.rs).
    gm_reply: Option<(u32, u16, Vec<u8>, bool)>,
    faq_report: u8,
    /// Social row MacroPalette's windows, mcrselec's page (its object is permanent) and mcrledit's line clipboard.
    macroedit: Option<macroedit::MacroEdit>,
    mcr_page: usize,
    /// The equipment sets (client/equipsets.rs), made at the first field entry.
    equipsets: Option<equipsets::EquipSets>,
    mcr_line: Option<Vec<u8>>,
    /// 0x067 kind 2 @0x24: our Mog House is open to visitors (FUN_100f96e0).
    mh_open: bool,
    /// s2c 0x02E arrived (DAT_1047faae), the moogle menu open (myroom / mogext and its storage menu).
    mog_pending: bool,
    moogle: Option<moogle::Menu>,
    /// s2c 0x01B, the job change request (the Mog House / nomad object; gone at zone-in) and its 3 s cooldown
    /// (myroom +0x18).
    job_info: Option<moogle::JobInfo>,
    job_req: Option<moogle::JobReq>,
    job_cooldown: Option<Instant>,
    /// The delivery box: s2c 0x04B's slots and the send / receive UIs.
    post: post::Session,
    /// An NPC shop's windows (s2c 0x03E).
    shop: Option<shop::Shop>,
    /// The NPC hand-over window (trade.rs).
    handover: Option<trade::Handover>,
    /// Player trade (trade.rs): the request, both windows and both offers.
    p2p: trade::P2p,
    /// The guild shop's buy / sell lists (s2c 0x083 / 0x085), cleared by 0x086 only.
    guild: [shop::GuildTable; 2],
    /// The mouse pick: actors whose screen rect held the pointer last test (their bones are read back), and the
    /// read-back points (key: bone, or 0x8000 | attach point) relative to the actor root.
    pick_rect: HashSet<u32>,
    pick_points: HashMap<u32, HashMap<u16, Vec3>>,
    /// Synthesis: tskill1, the request state, the result lines and the animations.
    synth: synth::State,
    /// Party join requests: the history (pec.dat) and the blacklist (client/partyreq.rs).
    partyreq: partyreq::State,
    /// s2c 0x044 blocks (main, sub), the BLU stamp and reset timer, the Set Spells screen (client/bluset.rs).
    ext_job: [[u8; 0x9c]; 2],
    blu_stamp: u32,
    blu_reset: Option<Instant>,
    blu_data: Option<Arc<crate::dat::blumagic::BluData>>,
    bluset: Option<bluset::Screen>,
    /// The server message's transfer, its one request per lobby login (first zone) still to come, and the world's
    /// name for its header.
    servmes: Option<servmes::Transfer>,
    servmes_armed: bool,
    world_name: String,
    /// The zone's ambient loop chosen this frame (Effects::ambient).
    ambient: Option<u32>,
    /// Message events that came before the message tables loaded (poll_network replays them).
    held_text: Vec<NetEvent>,
    /// Equipped linkshells by number (1, 2, 3): the item's (index, bag), index 0 none (s2c 0x0E0).
    linkshells: [(u8, u8); 4],
    /// The treasure pool's slots (G+0x1951c) and its windows ("menu    loot" and the two beside it).
    treasure: [treasure::Slot; treasure::SLOTS],
    loot: Option<treasure::Loot>,
    res_rdy: ResRdy,
    /// The lobby reply's feature bits (u32 @0x24; 0x04..0x80 = Wardrobe 3..8).
    features: u32,
    /// When each spell recast (DAT_106671e0) and 0x119 ability timer ends; 0x119 seen (DAT_10485824 bit 0).
    spell_recast: HashMap<u16, Instant>,
    ability_recast: HashMap<u16, Instant>,
    recasts_seen: bool,
    /// "menu    casttime" (FUN_1012c1d0): total and remaining ticks of our cast.
    cast_bar: Option<(f32, f32)>,
    item_info: Option<Arc<items::ItemCache>>,
    msgshift: Option<Arc<crate::dat::msgshift::MsgShift>>,
    zone_text: Option<(u16, Arc<crate::dat::dialog::Dialog>)>,
    my_race: u8,
    inv: items::Inventory,
    win: Option<items::Win>,
    stats: Option<items::CharStats>,
    /// The player flags the item updates keep (+0x3c35c): 0x800 settled since the last update, 0x200000 storage seen.
    inv_flags: u32,
    /// s2c 0x01D's u32 @8 (+0x198cc): the containers whose items have all come, bit = container.
    inv_loaded: u32,
    /// s2c 0x062 skill_base (DAT_104856a4): craft ranks in bits 0..4.
    skill_base: Option<[u16; 64]>,
    /// The zone's NPC name list (FUN_10093bd0), for NPCs whose 0x00E carries no name.
    npc_names: HashMap<u32, String>,
    /// Event and battle motion files, loaded in the background.
    motion_files: actmotion::Loader,
    /// FFXiMain's options for this character (gamecfg), the open Config windows, and what the app must take
    /// over: the SE / music levels changed, the values to save.
    game: crate::gamecfg::Values,
    config: Option<configui::ConfigUi>,
    volume_dirty: bool,
    game_save: bool,
    /// Our Settings or Extras page when opened from the Config window, and a request to open one.
    settings: Option<settings::SettingsUi>,
    want_settings: Option<settings::Kind>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Confirm {
    Logout,
    Shutdown,
    /// LAB_101d6040 with no exit bit: "Exit your Mog House?" (group 0xc 0x2f, 0x2e in a Mog House city).
    LeaveRoom,
    /// The K.O. window's button (FUN_1012e030): group 0xe 0x46 "Return to Home Point?".
    HomePoint,
    /// FUN_10096c90 mode 3: group 0xe 0x60 "Accept Tractor?".
    Tractor,
    /// map0 Wide Scan while tracking: group 0xe 0x45 "Stop tracking current target\nand proceed with wide area scan?".
    StopTracking,
    /// Drop of an Ex / Rare / augmented item (group 0xa 0x30), the Linkshell's two more (group 9 0x157 / 0x158).
    Drop { index: u8, id: u16, stage: u8 },
    /// The action menu's Dismount (FUN_10210e30): group 0xe 0x4d "Dismount?"; Yes runs /dismount.
    Dismount,
    /// post2's Dispose (2) / Return (3): group 9 0x34 / 0x35, default No.
    Post(u8),
    /// Trade again on the player we asked (FUN_10151e40): group 0xc 0x2d "Cancel trade transaction?".
    CancelTrade,
    /// The Linkshell windows' prompts.
    Ls(linkshell::Ask),
    /// Level Sync on (group 10 0x71) or off (0x72) with a party member (FUN_10204af0, FUN_10204e50).
    Sync { id: u32, race: u8, off: bool },
    /// Help Desk Config's Cancel New Adventurer Status (group 9 0xd4).
    NewAdventurer,
    /// MacroPalette's book / set paste and delete prompts.
    Macro(macroedit::Ask),
    /// merityn (FUN_10210e30, its own instance): raise / lower a merit, raise a job point.
    Merit(merits::Ask),
    /// link7 / link8 / link9's Kick (FUN_101e6620): group 10 0x36.
    LinkKick(linklist::Kick),
    JobPoint(jobpoints::Ask),
    AlterEgo(aepoints::Ask),
    /// group 9 0x140 "Really reset request history?" (the Request toggle, scoption's Clear History).
    Request(partyreq::Ask),
    /// The Records of Eminence prompts (FUN_10232770).
    Roe(roe::Ask),
    /// bluinven's Remove All (FUN_10210e30): group 9 0xe4 "Remove all set blue magic spells?".
    BlueRemoveAll,
}

impl World {
    /// We stand in the zone and scene that are loaded (a Mog House keeps the zone number but loads another scene, so
    /// the old ground must not settle or move us meanwhile).
    fn in_scene(&self) -> bool {
        self.zone == Some(self.pos_zone) && self.loading.is_none()
    }

    /// FUN_10118870: the cursor child a window opened by name starts on (the one it last had; 1 the first time).
    fn cursor_recall(&self, name: &str) -> u16 {
        self.cursor_mem.get(name).copied().unwrap_or(1)
    }

    /// FUN_10118830 on every hide or close, here every frame: the open windows' cursor children into their layouts.
    fn cursor_keep(&mut self) {
        if let Some(k) = self.menu {
            self.menu_mem[self.menu_page as usize & 1] = k;
        }
        let mut kept: Vec<(&'static str, u16)> = Vec::new();
        if let Some(s) = &self.status {
            kept.push((status::STATCOM, s.sel));
        }
        if let Some(m) = &self.partymenu {
            kept.extend(m.stack.iter().map(|win| (win.name, win.sel)));
        }
        if let Some(m) = &self.searchmenu {
            kept.extend(m.stack.iter().map(|win| (win.name, win.sel)));
        }
        if let Some(sel) = self.missions.as_ref().and_then(|m| m.menu) {
            kept.push((missions::MENU, sel));
        }
        if let Some(c) = &self.cats {
            kept.push((c.menu, magic::cat_child(c)));
        }
        self.cursor_mem.extend(kept);
    }

    fn dead(&self) -> bool {
        matches!(self.my_motion.status, 2 | 3)
    }

    /// Key item `id` is owned (s2c 0x055 bits, 512 per table).
    fn has_key_item(&self, id: u16) -> bool {
        self.key_items.get((id >> 5) as usize).is_some_and(|word| word & (1 << (id & 31)) != 0)
    }
}

enum Phase {
    /// `focus`: 0 server, 1 account, 2 password, 3 one-time code, 4 log in, 5 create account.
    Login { password: String, otp: String, focus: usize },
    Characters {
        lobby: Option<Lobby>,
        chars: Vec<Character>,
        new_name: String,
        race: usize,
        face: u8,
        size: u8,
        job: usize,
        nation: usize,
        selected: Option<usize>,
        screen: LobbyScreen,
        /// Highlighted main-menu item.
        menu: usize,
        /// Character awaiting delete confirmation.
        confirm: Option<usize>,
        /// Expansions the server has enabled (lobby 0x05 bitmask).
        expansions: u32,
    },
    World(Box<World>),
}

impl Phase {
    /// The credentials screen, with the cursor on the first field still to fill in.
    fn login(cfg: &Config) -> Self {
        Phase::Login { password: String::new(), otp: String::new(), focus: if cfg.username.is_empty() { 1 } else { 2 } }
    }
}

struct Gfx {
    window: Arc<Window>,
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    depth: wgpu::TextureView,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
}

/// Scripted run for testing: log in, pick a character, optionally walk, screenshot, log out.
#[derive(Default)]
pub struct Drive {
    pub login: Option<(String, String)>,
    pub character: Option<String>,
    pub walk_secs: f32,
    /// Waypoints (x, z) to run through in order once in the world.
    pub goto: Vec<(f32, f32)>,
    pub screenshot: Option<std::path::PathBuf>,
    pub screenshot_after: f32,
    /// Log out and close after the screenshot (otherwise the window stays up).
    pub quit: bool,
    /// Scripted input; with it the login is typed by the script instead of done directly.
    pub input: Option<script::Script>,
    /// While engaged, walk up to the target (test drives of melee).
    pub chase: bool,
    /// The zone the waypoints belong to; zoning drops the rest.
    pub(crate) goto_zone: Option<u16>,
    pub(crate) started: bool,
    pub(crate) zone_ready_at: Option<Instant>,
    pub(crate) shot_taken: bool,
    /// `act:relogin`: the next title opens Select Character, and the character is picked no sooner than
    /// `pick_after` (time to photograph the select screen).
    pub(crate) relogin: bool,
    pub(crate) pick_after: Option<Instant>,
}

impl Drive {
    /// A test run (credentials, a script or a screenshot given): not the user's own session.
    pub fn driven(&self) -> bool {
        self.login.is_some() || self.input.is_some() || self.screenshot.is_some()
    }
}

pub struct App {
    cfg: Config,
    drive: Drive,
    /// The chat line's history between logins (FSTEXTINPUT lives for the whole run).
    line_hist: linehist::LineHistory,
    gfx: Option<Gfx>,
    phase: Phase,
    status: String,
    busy: bool,
    tasks_tx: Sender<Task>,
    tasks: Receiver<Task>,
    keys: HashSet<KeyCode>,
    dragging: bool,
    left_down: bool,
    middle_down: bool,
    /// Where the right button went down (the camera arrow's reference).
    drag_from: Option<(f64, f64)>,
    /// FUN_10157fa0's +0x57: this movement push already sent its stand-up command.
    stand_sent: bool,
    /// Wheel notches since the last frame (FOV zoom), and those a focused list takes.
    wheel: f32,
    list_wheel: f32,
    mods: winit::keyboard::ModifiersState,
    press_at: Option<(f64, f64)>,
    /// When the left button went down, and the right one (where and when): the drag rule (FUN_10125040).
    press_time: Option<Instant>,
    right_press: Option<((f64, f64), Instant)>,
    /// A world click confirmed or opened a window: this frame's click is not the windows' (FUN_10123ea0).
    swallow_click: bool,
    /// The window size the last frame drew at (the mouse pick's projection).
    window_size: [u32; 2],
    last_cursor: Option<(f64, f64)>,
    last_frame: Instant,
    /// The creator's nation step backdrop.
    nation: NationShow,
    /// When the next frame may draw (the frame limit; see about_to_wait).
    next_frame: Option<Instant>,
    /// The last '/' key pressed was the numpad one (egui maps both to Key::Slash).
    numpad_slash: bool,
    /// Retail key functions (DAT_103619f0 and FUN_1011ff30).
    input: input::Input,
    /// Frames per second, 0 = unlimited (config frame_limit), and the present mode must be picked again.
    frame_limit: u32,
    present_dirty: bool,
    /// The window's size or mode changed in the config.
    window_dirty: bool,
    /// A window size to ask for, and the frames to wait first (see apply_window).
    size_pin: std::cell::Cell<Option<(winit::dpi::PhysicalSize<u32>, u32)>>,
    /// Frames until the full-screen state is asked for again after the window first shows.
    refullscreen: std::cell::Cell<Option<u32>>,
    /// The monitor the window was last put on, and frames until a monitor move restores the mode.
    monitor_applied: std::cell::RefCell<String>,
    /// A monitor move in progress: (stage, frames to wait), see move_window.
    move_stage: std::cell::Cell<Option<(u8, u32)>>,
    /// Monitors (name, label) and GPUs for the settings screen.
    monitors: Vec<(String, String)>,
    gpus: Vec<String>,
    index: Option<Arc<DatIndex>>,
    /// Installed-content mask (ROM2..): title music and lobycwin's tracks (FUN_1023ae80).
    installed: u32,
    keys_table: Option<Arc<zone::Keys>>,
    knock_table: Option<[[f32; 3]; 8]>,
    search_words: Option<Arc<crate::dat::tables::SearchWords>>,
    blu_data: Option<Arc<crate::dat::blumagic::BluData>>,
    cnq_tables: Option<Arc<crate::dat::tables::CnqTables>>,
    /// Title art, buttons and font from the game's lobby DATs; egui stand-ins until loaded.
    game_ui: Option<GameUi>,
    game_ui_state: LoadState,
    /// Whether this frame's UI pass drew game UI (lobby art or the HUD).
    ui_drawn: bool,
    /// Event crossfade and fade passes over the scene.
    screen_fx: Option<crate::render::screenfx::ScreenFx>,
    /// The world's, in the scene's plain format.
    screen_fx_world: Option<crate::render::screenfx::ScreenFx>,
    effects_gpu: Option<crate::render::effects::EffectsGpu>,
    /// Lobby screen fades, and whether the window has been shown (it stays hidden until the title can draw).
    lobby_fade: LobbyFade,
    shown: bool,
    /// The title screen's 3D backdrop, and its loading state.
    flyover: Option<flyover::Flyover>,
    flyover_state: LoadState,
    /// The character select scene, its loading state and the character list it was built for.
    select: Option<select::SelectScene>,
    select_state: LoadState,
    select_key: String,
    /// The select / delete screen's cursor and windows.
    select_ui: select::SelectUi,
    /// The delete prompt's highlighted answer.
    delete_yes: bool,
    /// Character creator: preview scene, its loading state, the dancer being loaded, the step
    /// (race, face, hair, size, job, name, nation, confirm) and the confirm answer.
    creator: Option<creator::Creator>,
    creator_state: LoadState,
    dancer_loading: Option<(usize, usize)>,
    creator_step: usize,
    creator_yes: bool,
    /// HUD window rects this frame; clicks there do not pick in the world.
    hud_blockers: Vec<[f32; 4]>,
    /// Shut Down finished: close the window.
    exit_requested: bool,
    /// The window was asked to close in the world: a second ask with no logout running closes it at once.
    close_asked: bool,
    /// The pointer is over the window (edge panning).
    cursor_in: bool,
    /// The game's mouse cursor (client/mousecur.rs).
    mouse: mousecur::MouseCursor,
    /// A lobby or login error (or the new account's notice) waiting in the OK window on the login screen.
    login_error: Option<String>,
    /// The title's Config window (lobycwin) with its values, and our settings screen over the title.
    lobby_cfg: Option<configui::ConfigUi>,
    /// The title's pressed button (menu index), when, and whether the press was taken (loby2win +0x5e / +0x1c).
    title_press: Option<(usize, Instant, bool)>,
    lobby_values: crate::gamecfg::Values,
    lobby_settings: Option<settings::SettingsUi>,
    /// The start-up update checks (not retail) and the Update window over the title.
    updates: Option<updater::Shared>,
    /// No game files yet: the download window (not retail).
    first_run: Option<firstrun::FirstRun>,
    update_ui: Option<updater::UpdateUi>,
    /// UI layout size / window size (Config-tool 0037 / 0038).
    ui_k: (f32, f32),
    /// The offscreen scene target and its scaling pass (background resolution other than the window's).
    scene_scale: Option<crate::render::scale::SceneScale>,
    /// The top-left debug lines (position, zone, frame timing), toggled with F12.
    debug_overlay: bool,
    perf: perf::Perf,
    /// Sound output (opened on the first frame; None when there is no device).
    audio: Option<crate::audio::Audio>,
    audio_tried: bool,
    music_scene: MusicScene,
    /// A character was just created: the nation-step music (0x78) plays until the screen changes.
    music_created: bool,
}

/// What the music follows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MusicScene {
    None,
    /// Title music (FUN_1023ae50 on the installed mask).
    Title,
    Creator,
    Created,
    World,
}

/// Creator BGM per race (FFXiMain 0x1037cec0).
const CREATOR_BGM: [u32; 8] = [0x85, 0xa8, 0x84, 0x83, 0xa7, 0x82, 0xa6, 0xa5];

/// Menu state compared across a UI pass to pick the menu sound.
#[derive(Clone, PartialEq, Default)]
struct UiSnap {
    screen: Option<LobbyScreen>,
    cursor: Vec<usize>,
    /// The main menu's page (a swap plays 0014) and whether a row was picked (the menu closing then is no cancel).
    page: u8,
    picked: bool,
    confirm: bool,
    yes: bool,
    step: usize,
    menu_open: bool,
    world: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LoadState {
    Idle,
    Loading,
    Failed,
}

fn dir_from_forward(f: Vec3) -> u8 {
    // FFXI dir: 0 east, 64 south, 128 west, 192 north.
    let theta = (-f.z).atan2(f.x);
    ((theta / std::f32::consts::TAU * 256.0).rem_euclid(256.0)) as u8
}

fn forward_from_dir(dir: u8) -> Vec3 {
    let theta = dir as f32 / 256.0 * std::f32::consts::TAU;
    Vec3::new(theta.cos(), 0.0, -theta.sin())
}

impl App {
    pub fn new(mut cfg: Config, drive: Drive) -> Self {
        // A test drive types its own account (never saved over the user's, see login()).
        if let Some((user, _)) = &drive.login {
            cfg.username = user.clone();
        }
        let frame_limit = cfg.frame_limit();
        crate::render::ANISOTROPY.store(cfg.anisotropy.clamp(1, 16), std::sync::atomic::Ordering::Relaxed);
        crate::render::MIPMAPS.store(cfg.tool("0000") != 0, std::sync::atomic::Ordering::Relaxed);
        crate::render::UNCOMPRESSED.store(cfg.tool("0018") >= 2, std::sync::atomic::Ordering::Relaxed);
        let (tasks_tx, tasks) = channel();
        let phase = Phase::login(&cfg);
        let installed = crate::dat::installed(&cfg.ffxi_dir());
        let monitor = cfg.monitor.clone();
        // A test drive checks for updates only with FFXI_UPDATE_CHECK set.
        let drive_checks = drive.login.is_none() || std::env::var_os("FFXI_UPDATE_CHECK").is_some();
        let updates = (drive_checks && cfg.ffxi_dir().join("VTABLE.DAT").is_file()).then(|| {
            updater::tidy();
            updater::start(cfg.ffxi_dir())
        });
        Self {
            cfg,
            drive,
            line_hist: linehist::LineHistory::default(),
            gfx: None,
            phase,
            status: String::new(),
            busy: false,
            tasks_tx,
            tasks,
            keys: HashSet::new(),
            dragging: false,
            left_down: false,
            middle_down: false,
            drag_from: None,
            stand_sent: false,
            mouse: Default::default(),
            wheel: 0.0,
            list_wheel: 0.0,
            mods: Default::default(),
            press_at: None,
            press_time: None,
            right_press: None,
            swallow_click: false,
            window_size: [1, 1],
            last_cursor: None,
            last_frame: Instant::now(),
            next_frame: None,
            numpad_slash: false,
            input: input::Input::default(),
            frame_limit,
            present_dirty: false,
            window_dirty: false,
            size_pin: Default::default(),
            refullscreen: Default::default(),
            // The window is created on the configured monitor (full-screen modes), so nothing to move yet.
            monitor_applied: std::cell::RefCell::new(monitor),
            move_stage: Default::default(),
            monitors: Vec::new(),
            gpus: Vec::new(),
            nation: NationShow::default(),
            index: None,
            installed,
            keys_table: None,
            knock_table: None,
            search_words: None,
            blu_data: None,
            cnq_tables: None,
            game_ui: None,
            game_ui_state: LoadState::Idle,
            ui_drawn: false,
            screen_fx: None,
            screen_fx_world: None,
            effects_gpu: None,
            lobby_fade: LobbyFade::default(),
            shown: false,
            hud_blockers: Vec::new(),
            flyover: None,
            flyover_state: LoadState::Idle,
            select: None,
            select_state: LoadState::Idle,
            select_key: String::new(),
            select_ui: select::SelectUi::default(),
            delete_yes: false,
            creator: None,
            creator_state: LoadState::Idle,
            dancer_loading: None,
            creator_step: 0,
            creator_yes: true,
            exit_requested: false,
            close_asked: false,
            scene_scale: None,
            cursor_in: false,
            login_error: None,
            lobby_cfg: None,
            title_press: None,
            lobby_values: Default::default(),
            lobby_settings: None,
            updates,
            first_run: None,
            update_ui: None,
            ui_k: (1.0, 1.0),
            // FFXI_DEBUG_OVERLAY=1 starts with the F12 lines on (test shots).
            debug_overlay: std::env::var_os("FFXI_DEBUG_OVERLAY").is_some(),
            perf: Default::default(),
            audio: None,
            audio_tried: false,
            music_scene: MusicScene::None,
            music_created: false,
        }
    }

    /// Puts changed settings into effect: window size and mode, frame limit, anisotropy (new samplers), volumes.
    /// The background resolution is read every frame.
    fn apply_settings(&mut self) {
        self.frame_limit = self.cfg.frame_limit();
        self.present_dirty = true;
        crate::render::ANISOTROPY.store(self.cfg.anisotropy.clamp(1, 16), std::sync::atomic::Ordering::Relaxed);
        crate::render::MIPMAPS.store(self.cfg.tool("0000") != 0, std::sync::atomic::Ordering::Relaxed);
        crate::render::UNCOMPRESSED.store(self.cfg.tool("0018") >= 2, std::sync::atomic::Ordering::Relaxed);
        let (m, e) = self.volume_factors();
        if let Some(a) = self.audio.as_mut() {
            a.set_volumes(m, e);
            a.max_se = self.cfg.tool("0029").clamp(1, 20);
        }
        // 0007: sound off drops the device; on opens it again.
        if self.cfg.tool("0007") == 0 {
            self.audio = None;
        } else if self.audio.is_none() {
            self.audio_tried = false;
        }
        if let Phase::World(w) = &mut self.phase {
            w.party_tp = self.cfg.party_tp;
            w.map_opacity = self.cfg.map_overlay_opacity;
            w.map_overlay = self.cfg.map_overlay;
            w.synth_amount = self.cfg.synth_amount;
            w.stamp_all_lines = self.cfg.stamp_all_lines;
        }
        // Applied at the next frame start: during the UI pass the window is out of self.gfx.
        self.window_dirty = true;
    }

    /// Window size and mode (0001 / 0002 / 0034) on the live window. Test runs stay windowed unless
    /// FFXI_TEST_WINDOW is set.
    fn apply_window(&self, window: &Window) {
        if self.drive.driven() && std::env::var_os("FFXI_TEST_WINDOW").is_none() {
            return;
        }
        window.set_resizable(true);
        let (size, mode) = (self.cfg.window_size(), self.cfg.window_mode());
        // A new monitor: Wayland can't move a window and KWin won't switch a full-screen window's output, so go
        // full screen on the new one (that places the window there), then come back to the mode a few frames on.
        if *self.monitor_applied.borrow() != self.cfg.monitor {
            *self.monitor_applied.borrow_mut() = self.cfg.monitor.clone();
            if pick_monitor(&self.cfg.monitor, None, window.available_monitors(), size).is_some() {
                log::info!("window: moving to monitor {:?}", self.cfg.monitor);
                // Out of full screen first; the next stages run a few frames apart (move_window).
                window.set_fullscreen(None);
                self.move_stage.set(Some((1, 4)));
                return;
            }
        }
        let want = fullscreen(mode, size, pick_monitor(&self.cfg.monitor, window.current_monitor(), window.available_monitors(), size));
        if format!("{:?}", want) != format!("{:?}", window.fullscreen()) {
            window.set_fullscreen(want.clone());
        }
        // Full screen takes the monitor's size: asking for a size (or un-maximizing) would drop it out again.
        if want.is_some() {
            self.size_pin.set(None);
            return;
        }
        // Each of these makes a Wayland compositor send a configure with the old size, which would undo a
        // resize asked for in the same frame: change them only when needed, and ask for the size a few frames on.
        if window.is_decorated() != (mode == 1) {
            window.set_decorations(mode == 1);
        }
        if matches!(mode, 1 | 2) {
            if window.is_maximized() {
                window.set_maximized(false);
            }
            let want = winit::dpi::PhysicalSize::new(size[0], size[1]);
            if window.inner_size() != want {
                self.size_pin.set(Some((want, 4)));
            }
        }
    }

    /// The rest of a monitor move, a few frames per stage: 1 full screen on the new monitor (that places the
    /// window there), 2 the configured mode, without leaving full screen when that mode is full screen too.
    fn move_window(&self, window: &Window) {
        let Some((stage, frames)) = self.move_stage.get() else { return };
        if frames > 0 {
            self.move_stage.set(Some((stage, frames - 1)));
            return;
        }
        let size = self.cfg.window_size();
        let monitor = pick_monitor(&self.cfg.monitor, None, window.available_monitors(), size);
        match stage {
            1 => {
                window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(monitor)));
                self.move_stage.set(Some((2, 8)));
            }
            _ => {
                self.move_stage.set(None);
                if fullscreen(self.cfg.window_mode(), size, monitor).is_none() {
                    window.set_fullscreen(None);
                }
                self.apply_window(window);
            }
        }
    }

    /// The delayed half of apply_window: asks for the size once the frame counter runs out. A size granted at
    /// once (Wayland) comes without a Resized event, so the surface follows here, or the next frame's buffer
    /// would set the window back.
    fn request_size(&self, gfx: &mut Gfx) {
        let Some((want, frames)) = self.size_pin.get() else { return };
        if frames > 0 {
            self.size_pin.set(Some((want, frames - 1)));
            return;
        }
        self.size_pin.set(None);
        let got = gfx.window.request_inner_size(want);
        log::info!("window: asked for {}x{}, got {got:?}", want.width, want.height);
        if let Some(size) = got {
            resize_surface(gfx, size);
        }
    }

    /// Back to the login screen. The world entry (or the world) left the lobby faded out, so the fade starts over;
    /// an error waits in the OK window.
    fn to_login(&mut self, error: bool) {
        if let Phase::World(w) = &mut self.phase {
            self.line_hist = std::mem::take(&mut w.line_hist);
        }
        self.phase = Phase::login(&self.cfg);
        self.lobby_fade = LobbyFade::default();
        self.close_asked = false;
        // The character's window style (config 0x12) replaced lobby images (the select screen drew gray).
        if let Some(gu) = self.game_ui.as_mut() {
            gu.reset_style();
        }
        if error {
            log::warn!("back to the login screen: {}", self.status);
            self.login_error = Some(self.status.clone());
        }
    }

    /// Retail's levels under ours: in game ids 10 / 9 (FUN_10036480 / FUN_10035060), in the lobby 0xa8 for both
    /// (FUN_10196390).
    fn volume_factors(&self) -> (f32, f32) {
        match &self.phase {
            Phase::World(w) => world_volumes(w),
            _ => {
                let v = self.cfg.game.global.get(0xa8) as f32 / 100.0;
                (v, v)
            }
        }
    }

    fn ui_snap(&self) -> UiSnap {
        match &self.phase {
            Phase::Characters { screen, menu, selected, confirm, race, face, size, job, nation, .. } => {
                let create = *screen == LobbyScreen::Create;
                UiSnap {
                    screen: Some(*screen),
                    cursor: vec![*menu, selected.unwrap_or(99), *race, *face as usize, *size as usize, *job, *nation],
                    page: 0,
                    picked: false,
                    confirm: confirm.is_some() || (create && self.creator_step == 7),
                    yes: if create { self.creator_yes } else { self.delete_yes },
                    step: if create { self.creator_step } else { 0 },
                    menu_open: false,
                    world: false,
                }
            }
            Phase::World(w) => UiSnap {
                screen: None,
                cursor: vec![w.menu.unwrap_or(99)],
                page: w.menu_page,
                picked: w.menu_picked,
                confirm: w.confirm.is_some(),
                yes: w.confirm.is_some_and(|c| c.1),
                step: 0,
                menu_open: w.menu.is_some(),
                world: true,
            },
            Phase::Login { .. } => UiSnap::default(),
        }
    }

    /// The menu sound for what changed in one UI pass ("0001".."0014" in DAT 0).
    fn ui_sound(a: &UiSnap, b: &UiSnap) -> Option<crate::audio::Ui> {
        use crate::audio::Ui;
        if a.world != b.world || a.screen.is_some() != b.screen.is_some() {
            return None;
        }
        if a.screen != b.screen {
            return Some(if b.screen == Some(LobbyScreen::Main) { Ui::Cancel } else { Ui::Select });
        }
        if a.menu_open != b.menu_open {
            // A picked row plays its own sound (0002, or 0004 when refused: FUN_10140400).
            if !b.menu_open && b.picked {
                return None;
            }
            return Some(if b.menu_open { Ui::Open } else if b.confirm { Ui::Select } else { Ui::Cancel });
        }
        if a.page != b.page && b.menu_open {
            return Some(Ui::Open);
        }
        if a.step != b.step {
            return Some(if b.step > a.step { Ui::Select } else { Ui::Cancel });
        }
        if a.confirm != b.confirm {
            return Some(if b.confirm || a.yes { Ui::Select } else { Ui::Cancel });
        }
        (a.cursor != b.cursor || a.yes != b.yes).then_some(Ui::Cursor)
    }

    /// Music for the current screen and the zone, the music task's ticks, and creator foley.
    fn update_audio(&mut self, dt: f32) {
        // Config-tool 0007 = 0: the sound system is never started (FUN_100347d0).
        if !self.audio_tried && self.cfg.tool("0007") != 0 {
            self.audio_tried = true;
            // FFXI_AUDIO_DEVICE overrides the config's device (test runs into a null sink).
            let device = std::env::var("FFXI_AUDIO_DEVICE").unwrap_or_else(|_| self.cfg.audio_device.clone());
            let (m, e) = self.volume_factors();
            self.audio = crate::audio::Audio::open(self.cfg.ffxi_dir(), Some(&device), m, e);
            if let Some(a) = self.audio.as_mut() {
                a.max_se = self.cfg.tool("0029").clamp(1, 20);
            }
            self.music_scene = MusicScene::None;
        }
        let Some(a) = self.audio.as_mut() else { return };
        let scene = match &self.phase {
            Phase::Login { .. } => MusicScene::Title,
            // FUN_1022ef00: the race BGM until the nation step (its step 7, where the dancer is also torn down).
            Phase::Characters { screen: LobbyScreen::Create, .. } if !self.music_created && self.creator_step < 6 => MusicScene::Creator,
            Phase::Characters { screen: LobbyScreen::Create, .. } => MusicScene::Created,
            Phase::Characters { screen: LobbyScreen::Select, .. } if self.music_created => MusicScene::Created,
            Phase::Characters { .. } => MusicScene::Title,
            Phase::World(_) => MusicScene::World,
        };
        if scene != MusicScene::Created {
            self.music_created = false;
        }
        if scene != self.music_scene {
            match scene {
                MusicScene::Title => a.title_music(self.cfg.game.global.get(0xaf), self.installed),
                // LAB_1022fedb: reaching the nation step plays 0x78 through the fade task; back on the dancer steps
                // the race BGM returns when the performance wraps (FUN_10036a90).
                MusicScene::Created => a.change_music(0x78, 0x7f, 0x7f, 0, 0, false),
                _ => {}
            }
            self.music_scene = scene;
        }
        if let Some(cr) = &mut self.creator {
            if let Some(race) = cr.restarted.take()
                && scene == MusicScene::Creator
            {
                a.play_music_now(CREATOR_BGM[race.min(7)]);
            }
            for id in cr.sounds.drain(..) {
                a.play_se(id, 127);
            }
        }
        // The zone's ambient loop, only in the world (world+0x20 is 0 elsewhere).
        a.ambient(if let Phase::World(w) = &self.phase { w.ambient } else { None });
        if let Phase::World(w) = &mut self.phase {
            for (mask, level, ticks) in w.audio_cmds.drain(..) {
                a.event_volume(mask, level, ticks);
            }
            if !event::active(w) {
                w.event_voices = false;
            }
            for (v, t) in w.music_fades.drain(..) {
                a.music_fade(v, t);
            }
            w.music_changing = a.music_changing();
            a.max_se = if w.event_voices { 20 } else { self.cfg.tool("0029").clamp(1, 20) };
            // Interface sounds (targeting, menus, prompts).
            for id in w.sounds.drain(..) {
                a.play_ui_se(id);
            }
            let hour = w.clock.map(|(t, at)| crate::dat::weather::vana_minute(t, at.elapsed().as_secs_f32()) as u32 / 60).unwrap_or(12);
            let party = w.party.party().count().max(1) as u32;
            let cx = crate::audio::MusicContext { status: w.my_motion.status, hour, party, party_battle: party_battle(w), myroom: w.myroom.is_some_and(|r| r.map != 0x2d4) };
            if let Some(c) = w.music.select(cx) {
                log::info!("music: {c:?}");
                if c.clear {
                    a.clear_positions();
                }
                a.change_music(c.id, c.vol, c.vol, c.fade_in, 0x3c, c.save);
            }
        }
        a.update(dt);
    }

    fn spawn(&mut self, job: impl FnOnce() -> Task + Send + 'static) {
        self.busy = true;
        let tx = self.tasks_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(job());
        });
    }

    /// What --quit waits for: the script run to its end, else the --screenshot shot.
    fn drive_done(&self) -> bool {
        self.drive.input.as_ref().map_or(self.drive.shot_taken, |s| s.finished())
    }

    fn game_data(&mut self) -> Result<(Arc<DatIndex>, Arc<zone::Keys>)> {
        if self.index.is_none() {
            self.index = Some(Arc::new(DatIndex::open(&self.cfg.ffxi_dir())?));
            let dll = std::fs::read(self.cfg.ffxi_dir().join("FFXiMain.dll"))?;
            self.keys_table = Some(Arc::new(zone::Keys::from_ffximain(&dll)?));
            self.knock_table = crate::dat::tables::knockback(&dll).ok();
            self.search_words = crate::dat::tables::search_words(&dll).map_err(|e| log::warn!("{e:#}")).ok().map(Arc::new);
            let blu_desc = crate::dat::msg::current_slot_file(&dll, 1, 0x1a);
            self.blu_data = Some(Arc::new(crate::dat::blumagic::BluData::load(self.index.as_ref().unwrap(), blu_desc)));
            self.cnq_tables = crate::dat::tables::conquest(&dll).map_err(|e| log::warn!("{e:#}")).ok().map(Arc::new);
            self.input.load(&dll);
        }
        Ok((self.index.clone().unwrap(), self.keys_table.clone().unwrap()))
    }

    fn login(&mut self, password: String, otp: String, create: bool) {
        // A stray space makes a different account (" lenon" vs "lenon"), so never send one.
        self.cfg.username = self.cfg.username.trim().to_string();
        // xi_connect silently drops requests it considers malformed, so check its rules here.
        let printable = |s: &str| s.bytes().all(|b| (0x20..0x7F).contains(&b));
        if self.cfg.username.is_empty() || self.cfg.username.len() > 16 || !printable(&self.cfg.username) {
            self.status = "The account name must be 1-16 plain ASCII characters.".into();
            return;
        }
        if password.is_empty() || password.len() > 32 || !printable(&password) {
            self.status = if password.is_empty() { "Enter your password.".into() } else { "Passwords must be 1-32 plain ASCII characters (no accents or symbols outside ASCII).".into() };
            return;
        }
        let cfg = self.cfg.clone();
        // Test drives log in with their own account; keep the user's saved one.
        if self.drive.login.is_none() {
            let _ = self.cfg.save();
        }
        if create {
            self.spawn(move || {
                let req = Request { username: &cfg.username, password: &password, ..Default::default() };
                Task::AccountCreated(auth::send(&cfg.server, cfg.auth_port, Command::CreateAccount, &req).map(|_| ()))
            });
            return;
        }
        self.spawn(move || {
            Task::LoggedIn((|| {
                let req = Request { username: &cfg.username, password: &password, otp: &otp, ..Default::default() };
                let Reply::LoggedIn { account_id, session_hash, .. } = auth::send(&cfg.server, cfg.auth_port, Command::Login, &req)? else {
                    anyhow::bail!("unexpected reply from the login server");
                };
                let mut lobby = Lobby::connect(&cfg.server, cfg.view_port, cfg.data_port, account_id, session_hash, &cfg.client_version)?;
                let chars = lobby.characters()?;
                Ok((lobby, chars))
            })())
        });
    }

    fn handle_task(&mut self, task: Task) {
        // The lobby art loads in the background; nobody is waiting on it.
        if !matches!(task, Task::GameUi(_) | Task::Flyover(_) | Task::Nation(..) | Task::Select(..) | Task::CreatorBackdrop(_) | Task::Dancer(_) | Task::Battle(_) | Task::Names(_) | Task::Actions(_) | Task::AreaLoaded(..) | Task::SubAreaLoaded(..)) {
            self.busy = false;
        }
        match task {
            Task::Names(res) => match (res, &mut self.phase) {
                (Ok((n, m)), Phase::World(w)) => {
                    log::info!("names loaded; text id map for {} zones", m.zones.len());
                    w.item_info = Some(Arc::new(items::ItemCache::new(n.clone())));
                    w.names = Some(n);
                    w.msgshift = Some(Arc::new(m));
                }
                (Err(e), _) => log::warn!("item names: {e:#}"),
                _ => {}
            },
            Task::Actions(res) => match (res, &mut self.phase) {
                (Ok(a), Phase::World(w)) => w.actions = Some(Arc::new(a)),
                (Err(e), _) => log::warn!("spell / ability records: {e:#}"),
                _ => {}
            },
            Task::Furniture(res) => match (res, &mut self.phase) {
                (Ok(l), Phase::World(w)) if w.furniture.current(l.seq) => w.furniture.ready = Some(l),
                (Err(e), _) => log::warn!("furniture: {e:#}"),
                _ => {}
            },
            Task::Battle(res) => match (res, &mut self.phase) {
                (Ok(b), Phase::World(w)) => w.battle = Some(Arc::new(b)),
                (Err(e), _) => log::warn!("battle messages: {e:#}"),
                _ => {}
            },
            Task::Select(Ok(data), key) => {
                self.select = Some(select::SelectScene::new(data));
                self.select_key = key;
                self.select_state = LoadState::Idle;
            }
            Task::CreatorBackdrop(Ok(b)) => {
                self.creator = Some(creator::Creator::new(b));
                self.creator_state = LoadState::Idle;
            }
            Task::CreatorBackdrop(Err(e)) => {
                log::warn!("creator backdrop: {e:#}");
                self.creator_state = LoadState::Failed;
            }
            Task::Dancer(res) => {
                self.dancer_loading = None;
                match (res, &mut self.creator) {
                    (Ok(d), Some(c)) => c.set_dancer(d),
                    (Err(e), _) => log::warn!("creator dancer: {e:#}"),
                    _ => {}
                }
            }
            Task::Select(Err(e), _) => {
                log::warn!("select scene: {e:#}");
                self.select_state = LoadState::Failed;
            }
            Task::Flyover(Ok(data)) => {
                self.flyover = Some(flyover::Flyover::new(data));
                self.flyover_state = LoadState::Idle;
            }
            Task::Nation(n, res) => {
                if self.nation.loading == Some(n) {
                    self.nation.loading = None;
                    match res {
                        Ok(data) => self.nation.fly = Some((n, flyover::Flyover::new(data).unfaded())),
                        Err(e) => log::warn!("nation flyover {n}: {e:#}"),
                    }
                }
            }
            Task::Flyover(Err(e)) => {
                log::warn!("title flyover: {e:#}");
                self.flyover_state = LoadState::Failed;
            }
            Task::GameUi(Ok(data)) => self.game_ui = Some(GameUi::new(data)),
            Task::GameUi(Err(e)) => {
                log::warn!("lobby UI: {e:#}");
                self.game_ui_state = LoadState::Failed;
            }
            Task::AccountCreated(Ok(())) => {
                // In the OK window, with the cursor then on Log in.
                self.status.clear();
                self.login_error = Some(format!("The account {} was created.\nYou can log in now.", self.cfg.username));
                if let Phase::Login { focus, .. } = &mut self.phase {
                    *focus = 4;
                }
            }
            Task::LoggedIn(Ok((lobby, chars))) | Task::Lobby(Ok((lobby, chars))) => {
                self.status.clear();
                let (new_name, race, face, size, job, nation) = match &self.phase {
                    Phase::Characters { new_name, race, face, size, job, nation, .. } => (new_name.clone(), *race, *face, *size, *job, *nation),
                    _ => (String::new(), 0, 0, 1, 0, 0),
                };
                let selected = chars.iter().position(|c| !c.is_empty_slot());
                // Coming back from create or delete lands on the character list; a fresh login on the title menu.
                let screen = match &self.phase {
                    Phase::Characters { screen: LobbyScreen::Main, .. } | Phase::Login { .. } => LobbyScreen::Main,
                    Phase::Characters { .. } => LobbyScreen::Select,
                    _ => LobbyScreen::Main,
                };
                let expansions = lobby.expansions;
                self.phase = Phase::Characters { lobby: Some(lobby), chars, new_name, race, face, size, job, nation, selected, screen, menu: 0, confirm: None, expansions };
            }
            Task::Selected(Ok(login)) => {
                match crate::net::map::compress::Codec::from_ffximain(&std::fs::read(self.cfg.ffxi_dir().join("FFXiMain.dll")).unwrap_or_default()) {
                    Ok(codec) => {
                        let net = net::spawn(login.clone(), self.cfg.username.clone(), codec);
                        self.status = format!("Connecting to {}:{}...", login.map.0, login.map.1);
                        let features = if let Phase::Characters { lobby: Some(l), .. } = &self.phase { l.features } else { 0 };
                        let world_name = match &self.phase {
                            Phase::Characters { chars, selected: Some(s), .. } => chars.get(*s).map(|c| c.world.clone()).unwrap_or_default(),
                            _ => String::new(),
                        };
                        let expansions = if let Phase::Characters { expansions, .. } = &self.phase { *expansions & self.installed } else { 0 };
                        // The effects' GPU side caches by the World's registry ids (library, texture, mesh); a new
                        // World starts a new registry, so the old session's cache would draw the wrong ones.
                        self.effects_gpu = None;
                        self.phase = Phase::World(Box::new(World {
                            features,
                            net,
                            zone: None,
                            loading: None,
                            renderer: None,
                            effects: crate::fx::Effects::new(),
                            feet: HashMap::new(),
                            contacts: HashMap::new(),
                            my_pet: 0,
                            area_lights: HashMap::new(),
                            ground: None,
                            name: login.name.clone(),
                            search_addr: login.search,
                            searches: Vec::new(),
                            search: None,
                            search_words: self.search_words.clone(),
                            known_zones: None,
                            inspect: inspect::Inspect::default(),
                            line_hist: std::mem::take(&mut self.line_hist),
                            my_id: login.char_id,
                            my_index: 0,
                            pos: Vec3::ZERO,
                            yaw: 0.0,
                            entities: HashMap::new(),
                            visible: HashSet::new(),
                            zone_lines: Vec::new(),
                            lifts: Default::default(),
                            bump: Default::default(),
                            riding: false,
                            ride_y: 0.0,
                            lift_wait: None,
                            zoning_since: None,
                            chat: Default::default(),
                            chat_input: String::new(),
                            chat_caret: None,
                            chat_anchor: None,
                            copied: None,
                            fep: None,
                            log: Default::default(),
                            hud_top: Default::default(),
                            tells: Vec::new(),
                            battle: None,
                            pending: Vec::new(),
                            rounds: HashMap::new(),
                            holds: HashMap::new(),
                            held_status: HashMap::new(),
                            noreact: HashMap::new(),
                            move_lock: None,
                            face_locks: HashMap::new(),
                            loads: HashMap::new(),
                            knocks: HashMap::new(),
                            action_menu: None,
                            menu_picked: false,
                            battle_prev: 0,
                            gauge: None,
                            buffs: [0xffff; 32],
                            buff_timers: None,
                            buff_cursor: 0,
                            my_team: 0,
                            my_plate: net::SelfPlate::default(),
                            my_sync: 0,
                            last_check: None,
                            block_help: false,
                            party_buffs: HashMap::new(),
                            held_menu: None,
                            cursor_mem: HashMap::new(),
                            menu_mem: [0; 2],
                            started: Instant::now(),
                            logging_out: false,
                            target: None,
                            locked: false,
                            cam: Default::default(),
                            lock_height: -1.5,
                            unlock_ticks: 0.0,
                            camst: camera::State::new(),
                            last_status: 0,
                            auto_lock: true,
                            models: HashMap::new(),
                            models_loading: HashSet::new(),
                            zone_uploading: None,
                            party_tp: self.cfg.party_tp,
                            party_tp_save: false,
                            item_search: false,
                            follow: None,
                            translating: false,
                            last_proposer: String::new(),
                            overlay: None,
                            map_opacity: self.cfg.map_overlay_opacity,
                            map_overlay: self.cfg.map_overlay,
                            synth_amount: self.cfg.synth_amount,
                            stamp_all_lines: self.cfg.stamp_all_lines,
                            target_since: Instant::now(),
                            cam_turn: (0.0, 0.0),
                            sounds: Vec::new(),
                            area_line: None,
                            pos_zone: 0,
                            unsettled: false,
                            inv_loaded: 0,
                            actors: HashMap::new(),
                            moving: false,
                            walk: false,
                            autorun: false,
                            autorun_dir: Vec3::ZERO,
                            move_tier: 0.0,
                            step_ticks: 0.0,
                            step_hist: [Vec3::ZERO; 4],
                            step_to: Vec3::ZERO,
                            draw_pos: Vec3::ZERO,
                            fade: 1.0,
                            my_motion: Default::default(),
                            vitals: None,
                            ko_deadline: 0,
                            ko_open: false,
                            homing: false,
                            home_sent: None,
                            ko_offer: 0,
                            raise_yes: true,
                            raise_hover: None,
                            ptr_dy: 0.0,
                            ko_wait: None,
                            clock: None,
                            compass_clock: true,
                            names_hidden: None,
                            bgm_mute: false,
                            se_mute: false,
                            play_time: None,
                            chat_prefix: "/say ".into(),
                            chat_mode: 2,
                            chat_prev: (2, "/say ".into()),
                            tell_name: None,
                            tell_hist: Vec::new(),
                            chatctrl: None,
                            journal: Default::default(),
                            quests: None,
                            roe: Default::default(),
                            keyitems: None,
                            ki_seen: Default::default(),
                            searchmenu: None,
                            linkshell: None,
                            ls_pending: None,
                            ls_opened: 0,
                            ls_msg: Default::default(),
                            ls_msg_seq: 0,
                            my_fellow: 0,
                            search_name: String::new(),
                            search_cond: Default::default(),
                            search_status0: 0,
                            arealist: None,
                            arealist_by_region: true,
                            seacom: Default::default(),
                            seacom_edit: None,
                            conquest: region::Data::new(self.cnq_tables.clone()),
                            region_menu: None,
                            cnq: None,
                            region_cmd: None,
                            logs: None,
                            missions: None,
                            mission_cmd: None,
                            expansions,
                            status: None,
                            merits: None,
                            jp_access: None,
                            merits_ui: None,
                            linklist: None,
                            blklist: None,
                            furniture: Default::default(),
                            pupequip: None,
                            merit_tables: None,
                            merit_list: Default::default(),
                            merit_stamp: 0,
                            merit_mem: 0,
                            jp_ui: None,
                            jp_tables: None,
                            ae_ui: None,
                            ae_tables: None,
                            ae_data: None,
                            jp_list: HashMap::new(),
                            jp_seen: false,
                            jp_jobs: None,
                            monstrosity: None,
                            real_level: None,
                            partymenu: None,
                            invite: None,
                            group_cb: None,
                            levelsync: None,
                            conf_sys: [0; 12],
                            client_version: self.cfg.client_version.clone(),
                            weather: 1,
                            weather_from: None,
                            zone_weather: None,
                            area: AreaSwitch::default(),
                            sub: Default::default(),
                            scene: None,
                            loading_scene: 0,
                            myroom: None,
                            room_flag: false,
                            party: Default::default(),
                            party_gauges: Vec::new(),
                            target_gauge: (0, 0),
                            gauge_acc: 0.0,
                            gauge_last: None,
                            self_flags: 0,
                            party_asked: false,
                            macros: None,
                            palette: (0, 0, 0, [0, 0]),
                            ui_hidden: false,
                            shot_request: None,
                            shot_count: (0, -1),
                            mogdoor: None,
                            leaving_room: false,
                            doors: Default::default(),
                            event_voices: false,
                            event_looks: Vec::new(),
                            my_ride: Default::default(),
                            rides: HashMap::new(),
                            music_fades: Vec::new(),
                            music_changing: false,
                            typing: false,
                            log_focus: 0,
                            log_active: false,
                            full_log: None,
                            map: None,
                            key_items: Vec::new(),
                            downloading: Some(Instant::now()),
                            menu: None,
                            menu_page: 0,
                            confirm: None,
                            shutdown: false,
                            music: Default::default(),
                            index: None,
                            event: None,
                            event_pending: None,
                            screen: Default::default(),
                            last_camera: None,
                            self_lock: None,
                            map_open: None,
                            map_cmds: Vec::new(),
                            point_masks: None,
                            scan: Default::default(),
                            info_win: None,
                            info_page: std::cell::Cell::new((0, 0, 1)),
                            event_ext: [0; 24],
                            event_mem: Default::default(),
                            event_end_at: None,
                            talk_lock: None,
                            audio_cmds: Vec::new(),
                            names: None,
                            actions: None,
                            spells_known: Vec::new(),
                            commands_known: Vec::new(),
                            magic: None,
                            list_memory: HashMap::new(),
                            ability: None,
                            cats: None,
                            prompt: None,
                            party_cursor: None,
                            focus: None,
                            mount_bits: None,
                            mount_recast: None,
                            emote_tree: None,
                            emote_path: Vec::new(),
                            emote_jobs: 0,
                            emote_chairs: 0,
                            ally_gauges: Vec::new(),
                            mog_zone: false,
                            mh_owner: 0,
                            mh_floors: false,
                            mh_seen: false,
                            viewhouse: None,
                            currencies: None,
                            unity: None,
                            unity_tables: Default::default(),
                            bazaar: None,
                            bazaar_msg: Default::default(),
                            bazaar_customers: 0,
                            helpdesk: None,
                            faq_counter: 0,
                            gm_reply: None,
                            faq_report: 0,
                            macroedit: None,
                            mcr_page: 0,
                            equipsets: None,
                            mcr_line: None,
                            mh_open: false,
                            mog_pending: false,
                            moogle: None,
                            job_info: None,
                            job_req: None,
                            job_cooldown: None,
                            post: post::Session::default(),
                            shop: None,
                            handover: None,
                            p2p: Default::default(),
                            guild: Default::default(),
                            pick_rect: HashSet::new(),
                            pick_points: HashMap::new(),
                            synth: synth::State::default(),
                            partyreq: partyreq::State::default(),
                            ext_job: [[0; 0x9c]; 2],
                            blu_stamp: 0,
                            blu_reset: None,
                            blu_data: self.blu_data.clone(),
                            bluset: None,
                            treasure: Default::default(),
                            loot: None,
                            linkshells: Default::default(),
                            held_text: Vec::new(),
                            ambient: None,
                            servmes: None,
                            servmes_armed: true,
                            world_name,
                            res_rdy: ResRdy::default(),
                            spell_recast: HashMap::new(),
                            ability_recast: HashMap::new(),
                            recasts_seen: false,
                            cast_bar: None,
                            item_info: None,
                            msgshift: None,
                            zone_text: None,
                            my_race: 0,
                            inv: Default::default(),
                            win: None,
                            stats: None,
                            skill_base: None,
                            inv_flags: 0,
                            npc_names: HashMap::new(),
                            motion_files: Default::default(),
                            game: self.cfg.game.for_character(Some(&login.name)),
                            config: None,
                            volume_dirty: true,
                            game_save: false,
                            settings: None,
                            want_settings: None,
                        }));
                        if let Phase::World(w) = &mut self.phase {
                            cfgapply::apply_all(w);
                        }
                        if let Ok((index, _)) = self.game_data() {
                            if let Phase::World(w) = &mut self.phase {
                                w.index = Some(index.clone());
                            }
                            let dll = self.cfg.ffxi_dir().join("FFXiMain.dll");
                            let tx = self.tasks_tx.clone();
                            let idx = index.clone();
                            std::thread::spawn(move || {
                                let b = std::fs::read(&dll).map_err(anyhow::Error::from).and_then(|d| crate::dat::battle::BattleText::load(&idx, &d));
                                let _ = tx.send(Task::Battle(b));
                            });
                            let tx = self.tasks_tx.clone();
                            let idx = index.clone();
                            std::thread::spawn(move || {
                                let _ = tx.send(Task::Actions(crate::dat::actions::Actions::load(&idx)));
                            });
                            let tx = self.tasks_tx.clone();
                            std::thread::spawn(move || {
                                let n = crate::dat::names::Names::load(&index, crate::dat::items::Lang::En).map(|n| (Arc::new(n), crate::dat::msgshift::MsgShift::bundled().verified(&index)));
                                let _ = tx.send(Task::Names(n));
                            });
                        }
                    }
                    Err(e) => self.status = format!("{e:#}"),
                }
            }
            Task::CharacterLoaded(key, result) => {
                let Phase::World(world) = &mut self.phase else { return };
                world.models_loading.remove(&key);
                match result {
                    Ok(ch) => {
                        let ch = Arc::new(ch);
                        // Its files' effect schedulers and generators, ahead of the first action.
                        let files: Vec<u32> = ch.files.iter().chain(&ch.part_files).copied().collect();
                        world.effects.preload(&files);
                        for a in world.actors.values_mut().filter(|a| a.key == key) {
                            a.character = Some(ch.clone());
                            a.gpu = None;
                            a.motion_names = None;
                        }
                        world.models.insert(key, ch);
                        // Looks no actor wears any more go once there are more than MODEL_CACHE.
                        if world.models.len() > MODEL_CACHE {
                            let used: HashSet<&String> = world.actors.values().map(|a| &a.key).collect();
                            let spare: Vec<String> = world.models.keys().filter(|k| !used.contains(k)).cloned().collect();
                            for k in spare.into_iter().take(world.models.len() - MODEL_CACHE) {
                                world.models.remove(&k);
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("model {key}: {e:#}");
                        for a in world.actors.values_mut().filter(|a| a.key == key) {
                            a.failed = true;
                        }
                    }
                }
            }
            Task::SubAreaLoaded(zone_id, id, result) => {
                let Phase::World(w) = &mut self.phase else { return };
                let Some((want, from_event)) = w.sub.loading.filter(|_| w.zone == Some(zone_id)) else { return };
                if want != id {
                    return;
                }
                w.sub.loading = None;
                match result {
                    Ok(l) => {
                        log::info!("sub-area {id} loaded");
                        if let (Some(gfx), Some(r)) = (&self.gfx, w.renderer.as_mut()) {
                            r.set_sub(&gfx.gpu, id, &l.geometry, l.textures, l.keys.clone());
                        }
                        w.effects.reg.light_keys = l.keys;
                        if let Some(lib) = l.fx {
                            w.effects.load_sub_area(lib);
                        }
                        w.sub.weather = l.weather;
                        if let Some(old) = w.ground.take() {
                            l.ground.keep_body(&old);
                            if w.sub.home.is_none() {
                                w.sub.home = Some(old);
                            }
                        }
                        w.ground = Some(l.ground);
                        w.sub.current = Some(id);
                        doors::reapply(w);
                        if !from_event {
                            let _ = w.net.commands.send(NetCommand::SubMap { state: 1, id: id as u16 });
                        }
                    }
                    Err(e) => {
                        log::warn!("sub-area {id}: {e:#}");
                        doors::reapply(w);
                    }
                }
            }
            Task::AreaLoaded(area, result) => {
                let Phase::World(world) = &mut self.phase else { return };
                world.area.loading = false;
                match result {
                    Ok((mut scene, ground, weather, geometry, fx)) if world.area.want == Some(area) => {
                        if world.area.home.is_none() {
                            world.area.home = Some((world.renderer.take(), world.ground.take(), world.zone_weather.take(), world.effects.zone_lib()));
                        }
                        world.zone_weather = weather;
                        if let Some(l) = fx {
                            world.effects.load_zone(l);
                            world.effects.set_weather(crate::dat::weather::weather_tag(world.weather), 0.0);
                        }
                        if let Some(gfx) = &self.gfx {
                            world.effects.reg.light_keys = geometry.light_keys.clone();
                            world.renderer = Some(ZoneRenderer::streamed(&gfx.gpu, geometry, std::mem::take(&mut scene.textures), gfx.config.format.remove_srgb_suffix()));
                        }
                        world.ground = Some(ground);
                        world.area.shown = Some(area);
                    }
                    Ok(_) => {}
                    Err(e) => log::warn!("event area {area}: {e:#}"),
                }
                if world.area.want == Some(area) {
                    world.area.want = None;
                    world.area.done = true;
                }
            }
            Task::ZoneLoaded(zone_id, map_no, result) => {
                let Phase::World(world) = &mut self.phase else { return };
                world.loading = None;
                world.furniture.reset();
                world.area = AreaSwitch::default();
                match result {
                    Ok((mut scene, ground, weather, geometry, (zone_fx, global_fx))) => {
                        let asked = world.sub.asked.take();
                        world.sub = subarea::SubArea::new(scene.regions.clone(), scene.collision.take(), geometry.light_keys.clone(), geometry.area_keys.clone());
                        world.sub.asked = asked;
                        world.zone_weather = weather;
                        if let Some(g) = global_fx {
                            world.effects.set_global(g);
                        }
                        if let Some(z) = zone_fx {
                            if let Some(keys) = self.keys_table.clone() {
                                world.effects.set_keys(keys);
                            }
                            if let Some(t) = self.knock_table {
                                world.effects.set_knock_table(t);
                            }
                            if let Some(index) = world.index.clone() {
                                world.effects.set_index(index);
                            }
                            world.effects.load_zone(z);
                            world.effects.set_weather(crate::dat::weather::weather_tag(world.weather), 0.0);
                        }
                        world.zone_lines = scene.regions.iter().filter(|r| r.name.starts_with(['z', 'Z']) && r.name.len() == 8).cloned().collect();
                        world.lifts = lifts::Lifts::load(&scene.regions);
                        // Its geometry and textures go up over the next frames; the indicator stays until then.
                        if let Some(gfx) = &self.gfx {
                            world.effects.reg.light_keys = geometry.light_keys.clone();
                            world.renderer = Some(ZoneRenderer::streamed(&gfx.gpu, geometry, std::mem::take(&mut scene.textures), gfx.config.format.remove_srgb_suffix()));
                            world.zone_uploading = Some(Instant::now());
                        }
                        world.ground = Some(ground);
                        world.zone = Some(zone_id);
                        world.scene = Some(map_no);
                        world.npc_names = world.index.as_ref().map(|i| npc_names(i, zone_id)).unwrap_or_default();
                        for (id, e) in world.entities.iter_mut() {
                            if let Some(n) = world.npc_names.get(id) {
                                e.name = n.clone();
                            }
                        }
                        self.status.clear();
                    }
                    Err(e) => {
                        world.downloading = None;
                        self.status = format!("Loading zone {zone_id}: {e:#}");
                    }
                }
            }
            Task::LoggedIn(Err(e)) | Task::AccountCreated(Err(e)) | Task::Selected(Err(e)) => {
                self.status = format!("{e:#}");
                self.to_login(true);
            }
            Task::Lobby(Err(e)) => {
                self.status = format!("{e:#}");
                self.to_login(true);
            }
        }
    }

    /// FUN_10178a20 / FUN_101787c0 each frame: the sub-area trigger under the player (or one asked for) loads its
    /// interior in the background, or 0 unloads it.
    fn update_sub_area(&mut self) {
        let Phase::World(w) = &mut self.phase else { return };
        if w.zone != Some(w.pos_zone) || w.loading.is_some() || w.area.shown.is_some() || w.ground.is_none() {
            return;
        }
        match w.sub.next(w.pos.to_array()) {
            Some(subarea::Step::Unload) => {
                log::info!("sub-area {:?} unloads", w.sub.current);
                if let Some(r) = w.renderer.as_mut() {
                    r.clear_sub();
                }
                w.effects.drop_sub_area();
                w.effects.reg.light_keys = w.sub.zone_keys.clone();
                if let Some(home) = w.sub.home.take() {
                    if let Some(old) = &w.ground {
                        home.keep_body(old);
                    }
                    w.ground = Some(home);
                }
                doors::reapply(w);
                (w.sub.current, w.sub.weather) = (None, None);
                let _ = w.net.commands.send(NetCommand::SubMap { state: 1, id: 0 });
            }
            Some(subarea::Step::Load(id, from_event)) => {
                let Some(zone_id) = w.zone else { return };
                log::info!("sub-area {id} loading (zone {zone_id})");
                w.sub.loading = Some((id, from_event));
                // FUN_10176dd0(0): doors are solid while it loads.
                if let Some(g) = &w.ground {
                    g.close_all_doors();
                }
                let (collision, keys, areas) = (w.sub.zone_collision.clone(), w.sub.zone_keys.clone(), w.sub.zone_areas.clone());
                match self.game_data() {
                    Ok((index, zkeys)) => {
                        let tx = self.tasks_tx.clone();
                        std::thread::spawn(move || {
                            let _ = tx.send(Task::SubAreaLoaded(zone_id, id, subarea::load(&index, &zkeys, id, collision, &keys, &areas)));
                        });
                    }
                    Err(e) => {
                        log::warn!("sub-area {id}: {e:#}");
                        if let Phase::World(w) = &mut self.phase {
                            w.sub.loading = None;
                        }
                    }
                }
            }
            None => {}
        }
    }

    /// Event op 0x34 / 0x35: the zone's own id brings its kept scene back at once; another area loads like a zone.
    fn switch_area(&mut self) {
        let Phase::World(w) = &mut self.phase else { return };
        let Some(area) = w.area.want.filter(|_| !w.area.loading) else { return };
        if w.area.shown == Some(area) || (w.area.shown.is_none() && w.zone == Some(area)) {
            (w.area.want, w.area.done) = (None, true);
            return;
        }
        if w.zone == Some(area)
            && let Some((renderer, ground, weather, fx)) = w.area.home.take()
        {
            (w.renderer, w.ground, w.zone_weather) = (renderer, ground, weather);
            if let Some(l) = fx {
                w.effects.switch_zone(l);
                w.effects.set_weather(crate::dat::weather::weather_tag(w.weather), 0.0);
            }
            if let Some(r) = &w.renderer {
                w.effects.reg.light_keys = r.light_keys.clone();
            }
            (w.area.shown, w.area.want, w.area.done) = (None, None, true);
            return;
        }
        w.area.loading = true;
        // A background load like the select scene's: not through spawn, which marks the app busy.
        match self.game_data() {
            Ok((index, keys)) => {
                let tx = self.tasks_tx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(Task::AreaLoaded(area, (|| {
                    let file = index.read(crate::dat::zone::file_id(area as u32))?;
                    let scene = zone::load_scene(&keys, &file)?;
                    let ground = ground::Ground::build(&scene);
                    let weather = crate::dat::weather::ZoneWeather::parse(&file).map_err(|e| log::warn!("area {area} weather: {e:#}")).ok();
                    let geometry = Arc::new(crate::render::ZoneGeometry::build(&scene));
                    let fx = crate::fx::lib::Library::load(crate::dat::zone::file_id(area as u32), file).map_err(|e| log::warn!("effects: area {area}: {e:#}")).ok().map(|mut l| {
                        l.prewarm(&keys);
                        l
                    });
                    Ok((scene, ground, weather, geometry, fx))
                    })()));
                });
            }
            Err(e) => {
                log::warn!("event area {area}: {e:#}");
                if let Phase::World(w) = &mut self.phase {
                    (w.area.loading, w.area.want, w.area.done) = (false, None, true);
                }
            }
        }
    }

    fn poll_network(&mut self) {
        let Phase::World(world) = &mut self.phase else { return };
        let mut load = None;
        let mut looks: Vec<(u32, Source)> = Vec::new();
        // Lines that need the message tables wait for them (they load on a thread at world start), in order.
        let mut evs: Vec<NetEvent> = if world.battle.is_some() { std::mem::take(&mut world.held_text) } else { Vec::new() };
        evs.extend(world.net.events.try_iter());
        for ev in evs {
            if world.battle.is_none()
                && matches!(ev, NetEvent::Battle(_) | NetEvent::SystemMessage { .. } | NetEvent::MessageStd { .. } | NetEvent::Emote { .. } | NetEvent::TrophyList { .. } | NetEvent::TrophySolution { .. })
            {
                world.held_text.push(ev);
                continue;
            }
            match ev {
                NetEvent::Zoning => {
                    // FUN_100955d0 asks only on the lobby login's first zone (state +0x10 cleared by a zone change).
                    world.servmes_armed = false;
                    // Leaving the field writes the macro files and the list orders (FUN_101ab7d0) and stops a running
                    // macro.
                    if let Some(mut m) = world.macros.take() {
                        m.mix.merge(&world.spells_known);
                        m.aix.merge(&world.commands_known);
                        m.save();
                    }
                    self.status = "Zoning...".into();
                    world.zoning_since = Some(Instant::now());
                    world.downloading = Some(Instant::now());
                }
                NetEvent::SelfLook(tbl) => looks.push((world.my_id, Source::Pc(look_from(tbl)))),
                NetEvent::ZoneIn { zone, pos, dir, char_id, index, name, look, motion, game_time, weather, music, event: login_event, area, sub_map, scene, myroom, room_flag, dead_counter, play_time, mog_zone, send_count } => {
                    world.mog_zone = mog_zone;
                    world.play_time = Some((play_time, Instant::now()));
                    world.search = None;
                    world.blklist = None;
                    partyreq::black_reset(world);
                    inspect::drop(world);
                    treasure::close(world);
                    world.chatctrl = None;
                    world.partymenu = None;
                    world.status = None;
                    world.missions = None;
                    world.quests = None;
                    world.keyitems = None;
                    world.searchmenu = None;
                    world.arealist = None;
                    world.seacom_edit = None;
                    world.linkshell = None;
                    (world.region_menu, world.cnq) = (None, None);
                    world.names_hidden = None;
                    world.res_rdy = ResRdy { send_count, since: Some(Instant::now()), ..ResRdy::default() };
                    world.myroom = myroom;
                    world.room_flag = room_flag;
                    (world.mh_seen, world.viewhouse, world.currencies, world.unity, world.bazaar) = (false, None, None, None, None);
                    world.helpdesk = None;
                    world.macroedit = None;
                    // Field entry reads every macro file (FUN_101ba030); LSB's content id is the character id.
                    let bt = world.battle.clone();
                    world.macros = Some(macros::Macros::load(&self.cfg.ffxi_dir(), world.my_id, Box::new(move |id| bt.as_ref().and_then(|b| b.group9(id)))));
                    if world.equipsets.is_none()
                        && let Some(dir) = world.macros.as_ref().map(|m| m.dir().to_path_buf())
                    {
                        world.equipsets = Some(equipsets::EquipSets::new(dir));
                    }
                    equipsets::field_entry(world);
                    if !world.synth.loaded
                        && let Some(dir) = world.macros.as_ref().map(|m| m.dir().to_path_buf())
                    {
                        synth::load_history(world, &dir);
                    }
                    if !world.partyreq.loaded
                        && let Some(dir) = world.macros.as_ref().map(|m| m.dir().to_path_buf())
                    {
                        partyreq::load(world, &dir);
                    }
                    // Any game-state change shows the UI again.
                    world.ui_hidden = false;
                    (world.mogdoor, world.leaving_room) = (None, false);
                    (world.moogle, world.mog_pending, world.job_req) = (None, false, None);
                    world.post = post::Session::default();
                    world.shop = None;
                    world.handover = None;
                    synth::zone_reset(world);
                    // 0x00A SubMapNumber (FUN_1018bac0): the interior we logged out or zoned in, once the zone is in.
                    world.sub.asked = (sub_map != 0).then_some((sub_map as u32, false));
                    world.area_line = area;
                    world.my_race = look_from(look).race;
                    world.my_index = index;
                    world.event = None;
                    world.event_pending = None;
                    world.screen = Default::default();
                    world.self_lock = None;
                    world.map_open = None;
                    world.talk_lock = None;
                    world.hud_top.enter();
                    // FUN_10095650 at the world start: the Unity chat setting (config 0xb2) and the job master display
                    // (config 0x99 bit 8 clear) go to the server again.
                    let _ = world.net.commands.send(NetCommand::Packet { id: 0x118, body: vec![world.game.get(0xb2) as u8, 0, 0, 0] });
                    let _ = world.net.commands.send(NetCommand::Packet { id: 0x11B, body: vec![(world.game.get(0x99) & 0x100 == 0) as u8, 0, 0, 0] });
                    world.map = None;
                    world.scan = Default::default();
                    // LSB re-sends every container after c2s 0x00C and empties the temporary items and the recycle
                    // bin on zoning, so slots kept from the last zone would go stale (retail's reset: inferred).
                    world.inv = items::Inventory { sizes: world.inv.sizes, sizes2: world.inv.sizes2, ..Default::default() };
                    if let Some(ev) = login_event {
                        event::queue(world, ev);
                    }
                    let _ = world.net.commands.send(NetCommand::StatusRequest(0));
                    world.music.zone_in(music);
                    if myroom.is_some() {
                        world.music.myroom();
                    }
                    log::info!("zone music {music:?}");
                    world.clock = Some((game_time, Instant::now()));
                    // 0x00A DeadCounter (@0xA4, 60 Hz): the end time from now when logging in or zoning while dead.
                    if dead_counter > 0 {
                        world.ko_deadline = (game_time as u64 + (dead_counter / 60) as u64).saturating_sub(360);
                    }
                    world.weather = weather;
                    world.weather_from = None;
                    let m = crate::dat::weather::vana_minute(game_time, 0.0);
                    log::info!("vana'diel {:02}:{:02}, weather {} ({weather})", (m / 60.0) as u32, m as u32 % 60, crate::dat::weather::weather_tag(weather));
                    world.my_motion = motion;
                    looks.push((char_id, Source::Pc(look_from(look))));
                    world.actors.retain(|id, _| *id == char_id);
                    world.zoning_since = None;
                    world.pos = Vec3::from(pos);
                    world.pos_zone = zone;
                    world.unsettled = true;
                    log::info!("zone-in: zone {zone} at {:?} dir {dir}", pos);
                    // FUN_1018b500 -> FUN_1001e4f0: the view shake stops at a zone load.
                    world.camst.shake = camera::Shake::default();
                    // FUN_1008abe0: the event volume channels back to 1.0 at zone-in.
                    world.audio_cmds.push((0xf, 1.0, 0.0));
                    let f = forward_from_dir(dir);
                    camera::face(world, f.x.atan2(f.z));
                    world.my_id = char_id;
                    world.name = name;
                    world.entities.clear();
                    // The zone's actors go, and with them their queued swings, messages and holds.
                    world.pending.clear();
                    world.rounds.clear();
                    world.holds.clear();
                    world.held_status.clear();
                    world.noreact.clear();
                    world.loads.clear();
                    world.knocks.clear();
                    let current = world.zone == Some(zone) && world.scene == Some(scene);
                    let pending = world.loading == Some(zone) && world.loading_scene == scene;
                    if !current && !pending {
                        world.loading = Some(zone);
                        world.loading_scene = scene;
                        load = Some((zone, scene));
                    } else if world.loading.is_none() && world.zone_uploading.is_none() {
                        world.downloading = None;
                    }
                }
                NetEvent::Entity(e) => {
                    if e.king {
                        world.res_rdy.kings.insert(e.id);
                    }
                    if e.id == world.my_id {
                        // 0x100982f6: a full update about us ends the post-event wait (+0x1fa = 0).
                        if e.complete() && world.self_lock.take().is_some() {
                            log::info!("event: our own full update came");
                        }
                        continue;
                    }
                    if e.despawn {
                        // 0x1009a04d: an event participant stays until the event ends.
                        if !event::holds(world, e.id) {
                            world.entities.remove(&e.id);
                            world.actors.remove(&e.id);
                        }
                        continue;
                    }
                    // 0x1009a0bf: a first update without the full mask is dropped and the actor asked for again
                    // (+0x12c bit 15, FUN_1008d280).
                    if !world.entities.contains_key(&e.id) && !e.complete() {
                        log::info!("partial first update {:#x} (mask {:#x}): asking again", e.id, e.mask);
                        let _ = world.net.commands.send(NetCommand::CharReq(e.index));
                        continue;
                    }
                    if let Some(tbl) = e.look {
                        looks.push((e.id, Source::Pc(look_from(tbl))));
                    }
                    if let Some(npc) = e.npc {
                        looks.push((e.id, Source::Npc(npc)));
                    }
                    let fresh = !world.entities.contains_key(&e.id);
                    let held = combat::held(world, e.id);
                    let entry = world.entities.entry(e.id).or_insert(Entity {
                        name: String::new(),
                        pos: Vec3::ZERO,
                        kind: e.kind,
                        hpp: 100,
                        dir: 0,
                        target: Vec3::ZERO,
                        motion: e.motion,
                        ratio: 0.0,
                        last_pos: Vec3::ZERO,
                        claim: 0,
                        mob: false,
                        index: e.index,
                        named: false,
                        plural: false,
                        slide: Vec3::ZERO,
                        remain: 0.0,
                        counter: e.counter,
                        yaw: event::rad_of(e.dir),
                        dir_seen: e.dir,
                        grounded: e.ground,
                        flags: e.status,
                        no_gauge: false,
                        no_plate: false,
                        gens_off: false,
                        untargetable: false,
                        invisible: false,
                        radar_off: false,
                        plate: net::PlateExtra::default(),
                        sync: 0,
                        monstrosity: 0,
                        sub: [0, 8, 0],
                        master: 0,
                        init_done: false,
                        door: 0,
                        lift: None,
                        active: 0.0,
                        no_bump: false,
                        model: 0,
                        object: false,
                        face: 0,
                        spawn_fx: false,
                        ride: Default::default(),
                        name_ncol: 0x17,
                        marks: net::Marks::default(),
                        female: false,
                        sub_kind: 0,
                    });
                    if e.mask & 7 != 0 {
                        entry.flags = e.status;
                    }
                    // Falling dead plays the death motion once (the "dead" scheduler), then the corpse loop.
                    // Fields outside this update's mask bits are zero, not news.
                    let mut motion = if fresh { e.motion } else { entry.motion };
                    if e.pos.is_some() {
                        (motion.speed, motion.speed_base) = (e.motion.speed, e.motion.speed_base);
                    }
                    if e.hpp.is_some() {
                        // A held actor (mid-swing) applies the change when its hold ends (combat::tick).
                        if !fresh && held {
                            world.held_status.insert(e.id, e.motion.status);
                        } else {
                            motion.status = e.motion.status;
                            world.held_status.remove(&e.id);
                        }
                    }
                    // GraphSize is in Flags1 (u32 @0x20), sent with SendFlg.General (0x04) only: a position update
                    // has it zero, which flipped monsters' sizes between packets.
                    if e.mask & 0x04 != 0 {
                        motion.graph_size = e.motion.graph_size;
                    }
                    let died = !fresh && matches!(motion.status, 2 | 3) && !matches!(entry.motion.status, 2 | 3);
                    let status_change = (!fresh && motion.status != entry.motion.status).then_some((entry.motion.status, motion.status));
                    entry.motion = motion;
                    entry.index = e.index;
                    if let Some(f) = e.face {
                        entry.face = f;
                    }
                    if let Some(s) = e.spawn_fx {
                        entry.spawn_fx = s;
                    }
                    if let Some(r) = e.ride {
                        entry.ride = r;
                    }
                    if let Some(m) = e.marks {
                        entry.marks = m;
                    }
                    if let Some(m) = e.male {
                        entry.female = !m;
                    }
                    if let Some((named, plural)) = e.name_flags {
                        (entry.named, entry.plural) = (named, plural);
                    }
                    if died && let Some(a) = world.actors.get_mut(&e.id) {
                        combat::die(a);
                    }
                    if let Some((old, new)) = status_change {
                        combat::status_schedulers(&mut world.effects, &world.actors, e.id, old, new);
                    }
                    if let Some(p) = e.pos {
                        let mut p = Vec3::from(p);
                        entry.grounded = e.ground;
                        if e.ground
                            && let Some(y) = world.ground.as_ref().filter(|_| world.zone == Some(world.pos_zone) && world.loading.is_none()).and_then(|g| g.snap(p.x, p.z, p.y))
                        {
                            p.y = y;
                        }
                        entry.target = p;
                        // FUN_1008b610: slide over the counter delta + 8 ticks (a smaller counter restarts it).
                        let c = e.counter;
                        let dur = if c < entry.counter { c } else { c - entry.counter };
                        entry.remain = dur as f32 + 8.0;
                        entry.counter = c;
                        if fresh {
                            entry.pos = p;
                            entry.last_pos = p;
                            entry.slide = p;
                            entry.remain = 0.0;
                        }
                        entry.dir = e.dir;
                        entry.dir_seen = e.dir;
                    }
                    // FUN_10093430 via the 0x00E handler: the packet's name where it carries one (players, index >= 0x700,
                    // text marked 0x01), else the zone list by server id (FUN_1008a930), else "NPC" (0x1035bb44).
                    if let Some(n) = e.name {
                        entry.name = n;
                    } else if let Some(n) = world.npc_names.get(&e.id) {
                        if entry.name != *n {
                            entry.name = n.clone();
                        }
                    } else if e.kind == EntityKind::Npc && e.index < 0x700 && entry.name.is_empty() {
                        entry.name = "NPC".into();
                    }
                    if let Some(h) = e.hpp {
                        entry.hpp = h;
                    }
                    if let Some(c) = e.claim {
                        entry.claim = c;
                    }
                    if let Some(m) = e.mob {
                        entry.mob = m;
                    }
                    if let Some(u) = e.untargetable {
                        entry.untargetable = u;
                    }
                    if let Some(x) = e.plate {
                        entry.plate = x;
                    }
                    if let Some((inv, off, mon)) = e.radar {
                        (entry.invisible, entry.radar_off, entry.monstrosity) = (inv, off, mon);
                    }
                    if let Some(s) = e.sub_anim {
                        entry.sub[0] = s;
                    }
                    if let Some((b20, b3)) = e.hp_bits {
                        (entry.no_gauge, entry.no_plate) = (b20 || b3, b3);
                    }
                    // The 0x00E look handler (jump table 0x1009ce88) ORs +0x128 bit 3 for doors, elevators and ships
                    // (SubKind 2..4) and for SubKind 0 models in fixed id ranges (asm 0x1009ca3c..0x1009caff): no gauge,
                    // no plate (FUN_101501a0, FUN_100ce4d0). The bit stays.
                    if e.kind == EntityKind::Npc {
                        let model_object = |m: u16| matches!(m, 0x32..=0x3b | 0x32e..=0x331 | 0x3c0..=0x3c9 | 0x737..=0x746 | 0x974..=0x97d | 0x9ba..=0x9c3);
                        if e.npc.is_some() {
                            entry.sub_kind = e.look_kind;
                        }
                        entry.object |= matches!(e.look_kind, 2..=4) || (e.look_kind == 0 && matches!(e.npc, Some(crate::dat::npc::NpcLook::Model(m)) if model_object(m)));
                        entry.model = match (e.look_kind, e.npc) {
                            (0, Some(crate::dat::npc::NpcLook::Model(m))) => m,
                            _ => 0,
                        };
                    }
                    if entry.object {
                        (entry.no_gauge, entry.no_plate) = (true, true);
                    }
                    if let Some(d) = e.door {
                        entry.door = d;
                    }
                    if e.lift.is_some() {
                        entry.lift = e.lift;
                    }
                }
                NetEvent::Chat { kind, attr, data, from, text } => {
                    // The block list (LAB_100e37a0 step 6): a blacklisted sender's line goes, but kind 0x19 and GM lines.
                    if kind != 0x19 && attr & 1 == 0 && partyreq::blacklisted(world, &from) == 1 {
                        continue;
                    }
                    // Tells (and kind 0x15) go into the reply history.
                    if matches!(kind, 3 | 0x15) && !from.is_empty() {
                        world.tells.retain(|t| *t != from);
                        world.tells.insert(0, from.clone());
                    }
                    // The yell's zone: string table 4 (our place names), else group 9 0x40.
                    let zone = if kind == 0x1a {
                        world.names.as_ref().and_then(|n| crate::dat::names::NameSource::place(&**n, data as u32)).filter(|z| !z.is_empty()).or_else(|| world.battle.as_ref().and_then(|b| b.group9(0x40))).unwrap_or_default()
                    } else {
                        String::new()
                    };
                    if let Some((line, mode)) = chat_line(kind, attr, data, &from, &text, &zone) {
                        push_chat_mode(world, line, mode, kind);
                    }
                }
                NetEvent::Logout { state } => {
                    log::info!("logged out of the zone (0x00B state {state})");
                    if let Some(dir) = world.macros.as_ref().map(|m| m.dir().to_path_buf()) {
                        synth::save_history(world, &dir);
                        partyreq::save(world, &dir);
                    }
                    if let Some(mut m) = world.macros.take() {
                        m.mix.merge(&world.spells_known);
                        m.aix.merge(&world.commands_known);
                        m.save();
                    }
                    // The server has ended the session (the net thread is done) whatever our toggle thinks: it cancels a
                    // countdown on its own (moving), so the local flag can be off when its logout comes.
                    self.status = "Logged out.".into();
                    self.exit_requested = world.shutdown;
                    self.to_login(false);
                    return;
                }
                NetEvent::StatusTimers { ids, ends } => world.buff_timers = Some((ids, ends)),
                NetEvent::PartyEffects(rows) => world.party_buffs = rows.into_iter().collect(),
                NetEvent::Battle(m) => combat::on_battle_message(world, &m),
                NetEvent::Event(s) => event::queue(world, s),
                NetEvent::EventParams(n) => event::params(world, n),
                NetEvent::EventRelease { mode, event: e } => event::release(world, mode, e),
                NetEvent::Wpos { id, pos, dir, mode } => {
                    // POSMODE (LSB): 0 normal, 1 event, 3 pop, 5 reset, 6 materialize move us; 0x0a turns.
                    // FUN_1009ee60: mode 7 about us unloads the sub-area.
                    if id == world.my_id && mode == 7 {
                        world.sub.asked = Some((0, false));
                    }
                    if id == world.my_id && matches!(mode, 0 | 1 | 3 | 5 | 6 | 0x0a) {
                        if mode != 0x0a {
                            world.pos = Vec3::from(pos);
                        }
                        let f = forward_from_dir(dir);
                        camera::face(world, f.x.atan2(f.z));
                    }
                    event::position_ack(world, mode);
                    // FUN_1008f740: entity +0x144 = Mode; 3 runs `pop0`, 6 `pop1` on the actor.
                    match mode {
                        3 => world.effects.on_scheduler(id, id, u32::from_le_bytes(*b"pop0")),
                        6 => world.effects.on_scheduler(id, id, u32::from_le_bytes(*b"pop1")),
                        _ => {}
                    }
                }
                NetEvent::ZoneText(t) => event::zone_text(world, &t),
                NetEvent::ItemMax(s, s2) => (world.inv.sizes, world.inv.sizes2) = (s, s2),
                NetEvent::Mounts(bits) => world.mount_bits = Some(bits),
                NetEvent::EmoteUnlocks { jobs, chairs } => (world.emote_jobs, world.emote_chairs) = (jobs, chairs),
                NetEvent::MountRecast(s) => world.mount_recast = (s > 0).then(|| Instant::now() + std::time::Duration::from_secs(s as u64)),
                // LAB_1009b6xx: the entity at ActIndex `index` with server id `id` takes its master.
                NetEvent::EntityMaster { index, id, master } => {
                    if let Some(e) = world.entities.get_mut(&id).filter(|e| e.index == index) {
                        e.master = master;
                    }
                }
                NetEvent::CharSync { id, owner, floors, open, level, sync, custom } => {
                    if id == world.my_id {
                        world.my_ride.custom = custom;
                    }
                    // @0x10 bit 1 Campaign (+0x1da), bit 2 Level Sync (+0x130 bit 23): name plate icons.
                    if id == world.my_id {
                        world.my_sync = sync;
                    } else if let Some(e) = world.entities.get_mut(&id) {
                        e.sync = sync;
                    }
                    if id == world.my_id {
                        world.real_level = Some(level);
                        world.mh_seen = true;
                    }
                    if id == world.my_id && (world.mh_owner, world.mh_floors, world.mh_open) != (owner, floors, open) {
                        log::info!("char sync: Mog House owner {owner:#x}, 2F {floors}, open {open}");
                        (world.mh_owner, world.mh_floors, world.mh_open) = (owner, floors, open);
                        if open && let Some(m) = world.moogle.as_mut() {
                            m.open_pending = false;
                        }
                    }
                }
                NetEvent::MogMenu => world.mog_pending = true,
                NetEvent::JobInfo(j) => world.job_info = Some(j),
                NetEvent::ExtJob(b) => bluset::ext_job(world, &b),
                NetEvent::Pbx(p) => post::on_pbx(world, p),
                // LAB_100a2760: the actor's jump flag (FUN_1008b410 -> FUN_100c63d0: 32 ticks of "jmp"). LSB also echoes it
                // to the jumper; retail's toggle would answer that with another 0x11D, so ours ignores our own (inferred).
                NetEvent::Jump(index) if index != world.my_index => {
                    if let Some(id) = world.entities.iter().find(|(_, e)| e.index == index).map(|(id, _)| *id) {
                        start_jump(world, id);
                    }
                }
                NetEvent::Jump(_) => {}
                NetEvent::SynthEffect { id, index, effect, kind, status, timer } => synth::on_effect(world, id, index, effect, kind, status, timer),
                NetEvent::SynthResult(a) => synth::on_result(world, a),
                NetEvent::SynthOthers(a) => synth::on_others(world, a),
                // FUN_100ec6c0: 0x01D State 1 sets the settled bits; each item update clears 0x800, and a storage
                // container's 0x01F / 0x020 sets 0x200000 (never cleared).
                NetEvent::ItemsSettled(state, loaded) => {
                    if state == 1 {
                        world.inv_flags |= 0x0020_0800;
                    }
                    world.inv_loaded = loaded;
                }
                NetEvent::ItemNum { container, index, count } => {
                    world.inv_flags &= !0x800;
                    world.inv.count(container, index, count)
                }
                NetEvent::Item { container, index, id, count, ext, lock, price } => {
                    world.inv_flags &= !0x800;
                    if matches!(container, 1..=7 | 9) {
                        world.inv_flags |= 0x0020_0000;
                    }
                    world.inv.set(container, index, id, count);
                    if id != 0 {
                        world.inv.locks.insert((container, index), lock);
                    }
                    if let Some(e) = ext.filter(|_| id != 0) {
                        world.inv.ext.insert((container, index), e);
                    }
                    match price {
                        Some(p) if id != 0 => _ = world.inv.prices.insert((container, index), p),
                        _ if id == 0 => _ = world.inv.prices.remove(&(container, index)),
                        _ => {}
                    }
                }
                NetEvent::Equip { slot, index, container } => world.inv.set_equip(slot, index, container),
                NetEvent::CharStats(s) => {
                    world.stats = Some(s);
                    status::on_stats(world);
                }
                NetEvent::Merits { limit, bits, max } => world.merits = Some((limit, bits, max)),
                NetEvent::JobPoints(access, jobs) => {
                    world.jp_access = Some(access);
                    world.jp_jobs = Some(jobs);
                }
                NetEvent::MeritList(recs) => merits::on_list(world, &recs),
                NetEvent::GmMessage { id, seq, last, text } => faqmain::on_message(world, id, seq, last, &text),
                NetEvent::JobPointList(recs) => jobpoints::on_list(world, &recs),
                NetEvent::AlterEgo(d) => aepoints::on_data(world, d),
                NetEvent::Monstrosity { kind, rank, prestige } => {
                    let m = world.monstrosity.get_or_insert((0, 0, 0));
                    m.0 |= 1 << kind;
                    if kind == 3 {
                        (m.1, m.2) = (rank, prestige);
                    }
                }
                NetEvent::SkillBase(s) => world.skill_base = Some(s),
                NetEvent::Action(a) => combat::on_action(world, &a),
                NetEvent::Scheduler { actor, name, .. } => combat::on_scheduler(world, actor, name),
                NetEvent::Status(s) => log::info!("net: {s}"),
                NetEvent::SystemMessage { id, para, para2 } => combat::on_system_message(world, id, [para as i32, para2 as i32]),
                NetEvent::Fragments(p) => servmes::on_fragment(world, &p),
                NetEvent::ShopOpen { count } => shop::on_open(world, count),
                NetEvent::ShopList { last, entries } => shop::on_list(world, last, &entries),
                NetEvent::ShopBuy { index, count } => shop::on_buy(world, index, count),
                NetEvent::ShopSell { price, slot, kind, count } => shop::on_sell(world, price, slot, kind, count),
                NetEvent::GuildOpen { stat, time } => shop::on_guild_open(world, stat, time),
                NetEvent::GuildList { sell, stat, entries } => shop::on_guild_list(world, sell, stat, &entries),
                NetEvent::GuildResult { sell, item, count, trade } => shop::on_guild_result(world, sell, item, count, trade),
                NetEvent::TradeRequest { id, index } => trade::on_request(world, id, index),
                NetEvent::TradeResponse { id, kind, index } => trade::on_response(world, id, kind, index),
                NetEvent::TradeTheirs { n, counter, item, slot, attr } => trade::on_their_slot(world, n, counter, item, slot, attr),
                NetEvent::TradeMine { n, item, slot, inv } => trade::on_my_slot(world, n, item, slot, inv),
                NetEvent::InspectEquip(p) => inspect::on_equip(world, &p),
                NetEvent::InspectMessage(p) => inspect::on_message(world, &p),
                NetEvent::KnownZones(z) => world.known_zones = Some(z),
                NetEvent::Conquest(p) => region::on_conquest(world, &p),
                NetEvent::Influence(p) => region::on_influence(world, &p),
                // G+0x9808 / +0x980f by linkshell number: its item's slot and bag.
                NetEvent::LinkshellEquip { num, index, bag } => {
                    if let Some(l) = world.linkshells.get_mut(num as usize) {
                        *l = (index, bag);
                    }
                    linkshell::on_equip(world, num, index, bag);
                }
                NetEvent::LinkshellMessage(p) => linkshell::on_message(world, &p),
                NetEvent::MessageStd { id, cas, para, strings } => combat::on_message_std(world, id, cas, para, strings),
                NetEvent::TrophyList(l) => treasure::on_list(world, l),
                NetEvent::TrophySolution { slot, judge, loot, loot_point, entry, entry_point, entry_flg, loot_name, entry_name } => {
                    treasure::on_solution(world, slot, judge, loot, loot_point, entry, entry_point, entry_flg, loot_name, entry_name)
                }
                NetEvent::Weather(w, minutes) => {
                    // The lighting blends from the old weather's environment to the new one over the change's time.
                    let secs = minutes as f32 * 2.4;
                    world.weather_from = (secs > 0.0 && w != world.weather).then(|| (world.weather, Instant::now(), secs));
                    world.weather = w;
                    // One Vana'diel minute is 2.4 s (FUN_101804d0's duration is u16 @0x0a x 3600 units).
                    world.effects.set_weather(crate::dat::weather::weather_tag(w), minutes as f32 * 2.4);
                }
                NetEvent::Assist { player, target: t } => target::assist(world, player, t),
                // FUN_10098710: tables 0..7 only; the owned bits, then the seen bits (keyitems::Seen).
                NetEvent::KeyItems { table, bits, seen } if table <= 7 => {
                    let at = table as usize * 16;
                    if world.key_items.len() < at + 16 {
                        world.key_items.resize(at + 16, 0);
                    }
                    world.key_items[at..at + 16].copy_from_slice(&bits);
                    world.ki_seen.packet(table as usize, &seen);
                }
                NetEvent::KeyItems { .. } => {}
                NetEvent::Music { slot, id } => world.music.set_slot(slot, id),
                NetEvent::SelfStatus { status, hpp, dead, pet, flags, icons, team, fellow, plate, ride } => {
                    (world.my_ride.chocobo, world.my_ride.mount) = ride;
                    world.my_plate = plate;
                    world.my_fellow = fellow;
                    world.self_flags = flags;
                    world.buffs = icons;
                    world.my_team = team;
                    // 0x037 handler: the end time = dc2 when ahead of now, else now + dc1 / 60, only with dc1 > 0; the
                    // dead state machine's deadline is 6 minutes before it.
                    world.ko_deadline = 0;
                    if dead.0 as i32 > 0 {
                        let now = vana_now(world);
                        let end = if dead.1 as u64 > now { dead.1 as u64 } else { now + (dead.0 / 60) as u64 };
                        world.ko_deadline = end.saturating_sub(360);
                    }
                    world.my_pet = pet;
                    // Held mid-swing: applied when the hold ends (the disengage waits for the swing).
                    if combat::held(world, world.my_id) {
                        world.held_status.insert(world.my_id, status);
                    } else {
                        world.held_status.remove(&world.my_id);
                        combat::apply_status(world, world.my_id, status);
                    }
                    if let Some(v) = &mut world.vitals {
                        v.3 = hpp;
                    }
                }
                NetEvent::Vitals { hp, mp, tp, hpp, mpp } => world.vitals = Some((hp, mp, tp, hpp, mpp)),
                NetEvent::PartyTable(entries) => {
                    world.party.refresh_self(world.my_id, world.my_index, &world.name, world.pos_zone);
                    world.party.table(&entries);
                    world.party_asked = false;
                }
                NetEvent::PartyList(u, create) => {
                    world.party.refresh_self(world.my_id, world.my_index, &world.name, world.pos_zone);
                    world.party.list(&u, world.my_id, create);
                }
                NetEvent::PartyAttr(u) => world.party.attr(&u),
                NetEvent::MagicKnown(bits) => {
                    // A spell learned mid-session (a blue spell, a scroll) shows in the log as its new bit.
                    if !world.spells_known.is_empty() {
                        let new: Vec<usize> = (0..bits.len() * 8).filter(|&i| bits[i / 8] >> (i % 8) & 1 != 0 && world.spells_known.get(i / 8).is_none_or(|b| b >> (i % 8) & 1 == 0)).collect();
                        if !new.is_empty() {
                            log::info!("0x0AA: spells now known {new:?}");
                        }
                    }
                    world.spells_known = bits;
                }
                NetEvent::CommandsKnown(bits) => world.commands_known = bits,
                NetEvent::ResOffer { id, kind } if id == world.my_id && world.dead() => {
                    log::info!("0x0F9 offer {kind}");
                    match kind {
                        1 => (world.ko_offer, world.raise_yes) = (2, true),
                        2 => {
                            world.ko_offer = 3;
                            world.confirm = Some((Confirm::Tractor, false));
                        }
                        _ => world.ko_offer = 0,
                    }
                }
                NetEvent::ResOffer { .. } => {}
                NetEvent::ConfSys(c) => world.conf_sys = c,
                NetEvent::MissionLog { port, data } => world.journal.record(port, data),
                NetEvent::RoeUnity { sparks, deeds, plaudits, rot } => {
                    (world.roe.sparks, world.roe.deeds, world.roe.plaudits, world.roe.rot) = (sparks, deeds, plaudits, rot);
                    world.roe.flags |= 1;
                }
                NetEvent::RoeActive(r) => {
                    world.roe.active = r;
                    world.roe.flags |= 2;
                    roe::active_changed(world);
                }
                NetEvent::RoeLog { offset, data } => world.roe.log_part(offset, &data),
                NetEvent::Invite { anon, kind, name, race } => partymenu::invited(world, anon, kind, name, race),
                NetEvent::PartyRequest { status, name } => partyreq::received(world, status, name),
                NetEvent::Blacklist { stat, entries } => partyreq::blacklist(world, stat, entries),
                NetEvent::BlackEdit { id, name, mode } => partyreq::black_result(world, id, name, mode),
                NetEvent::ItemSearch { item, flag } => textcmd::item_search_result(world, item, flag),
                NetEvent::PollStart { name, kind, text } => textcmd::poll_start(world, &name, kind, &text),
                NetEvent::PollState { votes, kind, state, questions, name, text } => textcmd::poll_state(world, votes, kind, state, questions, &name, &text),
                NetEvent::Translate { item, from, to, from_text, to_text } => textcmd::translate_result(world, item, from, to, &from_text, &to_text),
                NetEvent::GroupId(gid) => partymenu::on_group_id(world, gid),
                NetEvent::BazaarShopper { state, hide, name } => bazaar::on_shopper(world, state, hide, name),
                NetEvent::BazaarSale { count, item, name } => bazaar::on_sale(world, count, item, name),
                NetEvent::Currencies { mode, data } => currencies::on_packet(world, mode, &data),
                NetEvent::EquipSetValid(slots) => esedit::on_valid(world, slots),
                NetEvent::EquipSetRes { count, changed, equipped } => equipsets::on_res(world, count, &changed, &equipped),
                NetEvent::LockStyleFlag(v) => equipsets::on_flag(world, v),
                NetEvent::LockStyleError(ids) => equipsets::on_lock_error(world, &ids),
                NetEvent::UnityTable { week, d, data } => world.unity_tables.store(week, d, &data),
                NetEvent::PointMasks(m) => world.point_masks = Some(m),
                NetEvent::TrackEntry(e) => world.scan.on_entry(e),
                NetEvent::TrackPos { index, level, pos, state } => world.scan.on_pos(index, level, pos, state),
                NetEvent::TrackState(s) => {
                    // LAB_101fb790: the list's end clears the busy strip and keeps the results.
                    if world.scan.on_state(s) {
                        world.scan.busy = false;
                        if let Some(m) = world.map.as_mut() {
                            m.scan_done();
                        }
                    }
                }
                NetEvent::MapGroup { id, zone, pos } => {
                    world.scan.party_busy = false;
                    let own = world.party.own_flags;
                    let member = world.party.members().skip(1).find(|m| m.id == id).map(|m| (m.name.clone(), (m.flags ^ own) & 3 != 0));
                    let p = Vec3::from(pos);
                    let key = (zone == world.pos_zone).then(|| world.ground.as_ref().map(|g| g.map_key(p))).flatten();
                    if let (Some((name, alliance)), Some(m)) = (member, world.map.as_mut())
                        && m.zone() == zone
                    {
                        let same = key.is_none_or(|k| k == m.region.key as u8);
                        m.party_pos(&name, pos[0], pos[2], alliance, same);
                    }
                }
                NetEvent::EventAnswer { id, data } => {
                    if let Some(l) = world.event.as_mut() {
                        l.rt.answer(id, &data);
                    }
                }
                NetEvent::Registration { index, result } => {
                    log::info!("0x0BF registration {result} for index {index:#x}");
                    if let Some(l) = world.event.as_mut() {
                        l.rt.registration(result as u32);
                    }
                }
                NetEvent::Emote { caster, target, emote, param, mode } => combat::on_emote(world, caster, target, emote, param, mode),
                NetEvent::AbilityRecasts(timers) => {
                    world.recasts_seen = true;
                    let now = Instant::now();
                    world.ability_recast = timers.into_iter().filter(|t| t.1 > 0).map(|(id, s)| (id, now + std::time::Duration::from_secs(s as u64))).collect();
                }
                NetEvent::Error(e) => {
                    self.status = format!("Disconnected: {e}");
                    self.to_login(true);
                    return;
                }
            }
        }
        // Looks the event scripts changed (op 0xb6) load like the server's.
        looks.extend(std::mem::take(&mut world.event_looks));
        // Load a character model whenever an actor's look is new or changed.
        let mut wanted = Vec::new();
        for (id, source) in looks {
            if matches!(source, Source::Pc(l) if l.race == 0) {
                continue;
            }
            let key = format!("{source:?}");
            let spawn_fx = world.entities.get(&id).is_some_and(|e| e.spawn_fx);
            let actor = world.actors.entry(id).or_insert_with(Actor::new);
            if actor.key == key {
                continue;
            }
            actor.key = key.clone();
            // FUN_1008f740 / FUN_100c56e0: a 'spop' model starts at colour 0x00808080, invisible.
            if spawn_fx {
                (actor.alpha, actor.fade, actor.spop) = (0.0, None, true);
            }
            actor.source = Some(source);
            // A look already loaded is shared at once; one load serves every actor waiting for it.
            if let Some(m) = world.models.get(&key) {
                actor.character = Some(m.clone());
                actor.gpu = None;
                actor.motion_names = None;
            } else if world.models_loading.insert(key.clone()) {
                wanted.push((key, source));
            }
        }
        if !wanted.is_empty() {
            if let Ok((index, _)) = self.game_data() {
                let tx = self.tasks_tx.clone();
                std::thread::spawn(move || {
                    for (key, source) in wanted {
                        let model = match source {
                            Source::Pc(look) => crate::dat::character::load_character(&index, &look),
                            Source::Npc(look) => crate::dat::npc::load_npc(&index, &look),
                            Source::File(file) => crate::dat::npc::load_file_model(&index, file),
                        };
                        let _ = tx.send(Task::CharacterLoaded(key, model.map(|mut m| {
                            m.prepare();
                            m
                        })));
                    }
                });
            }
        }
        let Phase::World(_) = &self.phase else { return };
        if let Some((zone_id, map_no)) = load {
            self.status = format!("Loading zone {zone_id}...");
            let need_global = match &self.phase {
                Phase::World(w) => !w.effects.has_global(),
                _ => false,
            };
            match self.game_data() {
                Ok((index, keys)) => self.spawn(move || {
                    Task::ZoneLoaded(zone_id, map_no, (|| {
                        let file = index.read(crate::dat::zone::file_id(map_no as u32))?;
                        let scene = zone::load_scene(&keys, &file)?;
                        let ground = ground::Ground::build(&scene);
                        let weather = crate::dat::weather::ZoneWeather::parse(&file).map_err(|e| log::warn!("zone {zone_id} weather: {e:#}")).ok();
                        let geometry = Arc::new(crate::render::ZoneGeometry::build(&scene));
                        let global = need_global.then(|| index.read(0).and_then(|f| crate::fx::lib::Library::load(0, f))).and_then(|r| r.map_err(|e| log::warn!("effects: DAT 0: {e:#}")).ok());
                        let zone_fx = crate::fx::lib::Library::load(crate::dat::zone::file_id(map_no as u32), file).map_err(|e| log::warn!("effects: zone {zone_id}: {e:#}")).ok().map(|mut l| {
                            l.prewarm(&keys);
                            l
                        });
                        Ok((scene, ground, weather, geometry, (zone_fx, global)))
                    })())
                }),
                Err(e) => self.status = format!("{e:#}"),
            }
        }
        // Mog House furniture: the installed set changed (client/furniture.rs).
        let job = match &mut self.phase {
            Phase::World(w) => furniture::changed(w),
            _ => None,
        };
        if let Some((seq, want, map, floor)) = job
            && let Ok((index, keys)) = self.game_data()
        {
            self.spawn(move || Task::Furniture(furniture::load(&index, &keys, seq, &want, map, floor)));
        }
    }

    fn update_movement(&mut self, dt: f32) {
        let Phase::World(world) = &mut self.phase else { return };
        // c2s 0x015's facetarget: the current target's ActIndex.
        let target = world.target.and_then(|id| if id == world.my_id { Some(world.my_index) } else { world.entities.get(&id).map(|e| e.index) }).unwrap_or(0);
        if let Ok(mut me) = world.net.me.lock() {
            me.target = target;
        }
        combat::tick(world);
        ko_step(world);
        event::update(world, dt);
        if world.in_scene() {
            doors::update(world);
            lifts::update(world);
        }
        // Weapon-skill motion files and motions the effect system's schedulers asked for last frame.
        for (id, files) in std::mem::take(&mut world.effects.motion_files) {
            if let Some(a) = world.actors.get_mut(&id) {
                a.pending = Some(actmotion::Pending { files, name: String::new(), kind: actmotion::Kind::Skill });
            }
        }
        for (id, file) in std::mem::take(&mut world.effects.effect_motions) {
            if let Some(a) = world.actors.get_mut(&id)
                && !a.fx_files.contains(&file)
            {
                a.fx_files.push(file);
            }
        }
        for (id, file) in std::mem::take(&mut world.effects.effect_motions_done) {
            if let Some(a) = world.actors.get_mut(&id) {
                a.fx_files.retain(|f| *f != file);
                a.extra.retain(|(k, m)| !(*k == actmotion::Kind::Effect && m.id == file));
            }
        }
        // FUN_1012c0f0: the cast bar runs down and closes at 0.
        if let Some((_, left)) = world.cast_bar.as_mut() {
            *left -= (dt * 60.0).max(1.0 / 60.0);
            if *left <= 0.0 {
                world.cast_bar = None;
            }
        }
        // FUN_1008c480: a model's "init" once it is in ("inte" too for status 0x22; none seated, none fishing);
        // FUN_1008ef60: "ini<n>" when a monster's sub-animation changes (a worm burrows with ini1, comes up with init).
        const SUB: [[u8; 4]; 8] = [*b"init", *b"ini1", *b"ini2", *b"ini3", *b"init", *b"ini1", *b"ini2", *b"ini3"];
        let mut starts: Vec<(u32, [u8; 4])> = Vec::new();
        let mut stops: Vec<(u32, [u8; 4])> = Vec::new();
        for (id, e) in world.entities.iter_mut() {
            let ready = world.actors.get(id).is_some_and(|a| a.character.is_some());
            if !ready || e.door != 0 || matches!(e.sub_kind, 2..=4) {
                continue;
            }
            let status = e.motion.status;
            if !e.init_done {
                e.init_done = true;
                e.sub[2] = e.sub[0];
                if !matches!(status, 6 | 0x32 | 0x38 | 0x3f..=0x53) {
                    starts.push((*id, *b"init"));
                    if status == 0x22 {
                        starts.push((*id, *b"inte"));
                    }
                }
            }
            let (want, applied) = (e.sub[0], e.sub[1]);
            if (want == 4 && applied == 0) || (want == 0 && applied == 4) {
                e.sub = [want, want, want];
                continue;
            }
            let running = |name: [u8; 4]| world.effects.actor_scheduler_running(*id, u32::from_le_bytes(name));
            if want < 4 {
                if want == e.sub[2] && want == applied {
                    continue;
                }
                e.sub[2] = want;
                if e.mob && applied != 8 && !running(SUB[want as usize]) {
                    starts.push((*id, SUB[want as usize]));
                }
                e.sub[1] = want;
            } else if want != applied && e.mob && !running(SUB[want as usize]) {
                if applied != 8 {
                    stops.push((*id, SUB[applied as usize & 7]));
                }
                starts.push((*id, SUB[want as usize]));
                e.sub = [want, want, want];
            }
        }
        for (id, name) in stops {
            world.effects.stop_actor_scheduler(id, u32::from_le_bytes(name));
        }
        for (id, name) in starts {
            log::debug!(target: "motion", "actor {id:#x}: scheduler {}", String::from_utf8_lossy(&name));
            world.effects.on_scheduler(id, id, u32::from_le_bytes(name));
        }
        // FUN_100d1070: once the model's files are in, its 'spop' runs on it (actor files, then DAT 0).
        let spops: Vec<u32> = world.actors.iter_mut().filter(|(_, a)| a.spop && a.character.is_some()).map(|(id, a)| {
            a.spop = false;
            *id
        }).collect();
        for id in spops {
            world.effects.on_scheduler(id, id, u32::from_le_bytes(*b"spop"));
        }
        search::poll(world);
        world.line_hist.track(world.typing);
        inspect::frame(world);
        for (id, off) in std::mem::take(&mut world.effects.gens_off) {
            if let Some(e) = world.entities.get_mut(&id) {
                e.gens_off = off;
            }
        }
        for (id, ticks, speed) in std::mem::take(&mut world.effects.self_moves) {
            ride::self_move(world, id, ticks, speed);
        }
        // Scheduler op 0x29: the model colour toward an ARGB target.
        for (id, argb, frames) in std::mem::take(&mut world.effects.fades) {
            if let Some(a) = world.actors.get_mut(&id) {
                let f = actmotion::Fade::argb(a.alpha, a.rgb, argb, frames.round() as i32);
                if frames <= 0.0 {
                    (a.alpha, a.rgb, a.fade) = (f.target, f.rgb.map_or(a.rgb, |[_, to, _]| to), None);
                } else {
                    a.fade = Some(f);
                }
            }
        }
        // Skill actions: load holds end first, then op 0x20 / 0x59 holds, reactions and messages (fx/tasks.rs).
        for id in std::mem::take(&mut world.effects.loads) {
            combat::fx_load(world, id);
        }
        for id in std::mem::take(&mut world.effects.releases) {
            combat::fx_release(world, id);
        }
        for (id, ticks, kind) in std::mem::take(&mut world.effects.holds) {
            combat::fx_hold(world, id, ticks, kind);
        }
        for op in std::mem::take(&mut world.effects.weapons) {
            let engaged = if op.actor == world.my_id { world.my_motion.status == 1 } else { world.entities.get(&op.actor).is_some_and(|e| e.motion.status == 1) };
            if (op.engaged_only && !engaged) || op.n >= 16 {
                continue;
            }
            if let Some(a) = world.actors.get_mut(&op.actor) {
                let n = op.n as usize;
                a.wep_hide[n][op.byte as usize] = op.hide;
                if op.byte == 0 {
                    a.wep_show_at[n] = op.show_after.then(|| Instant::now() + std::time::Duration::from_secs_f32(op.ticks / 60.0));
                }
            }
        }
        for (id, op, set) in std::mem::take(&mut world.effects.motion_sets) {
            if let Some(a) = world.actors.get_mut(&id)
                && let Some(ch) = a.character.as_ref()
            {
                a.motion_names.get_or_insert(ch.motions).apply(op, set);
            }
        }
        for (code, c, zoom, ticks) in std::mem::take(&mut world.effects.screen_ops) {
            world.screen.fx_op(code, c, zoom, ticks);
        }
        for (id, op, ticks) in std::mem::take(&mut world.effects.locks) {
            let until = Instant::now() + std::time::Duration::from_secs_f32(ticks.max(0.0) / 60.0);
            if op == 0x2e {
                if id == world.my_id {
                    world.move_lock = Some(world.move_lock.map_or(until, |t| t.max(until)));
                }
            } else {
                let e = world.face_locks.entry(id).or_insert(until);
                *e = (*e).max(until);
            }
        }
        for r in std::mem::take(&mut world.effects.reacts) {
            combat::react_op(world, &r);
        }
        for g in std::mem::take(&mut world.effects.guards) {
            combat::guard_op(world, &g);
        }
        for k in std::mem::take(&mut world.effects.knocks) {
            combat::knock(world, &k);
        }
        for (a, k, which) in std::mem::take(&mut world.effects.action_prints) {
            combat::print_record(world, &a, k, which);
        }
        for (id, m) in std::mem::take(&mut world.effects.motions) {
            actmotion::play_op(world, id, m);
        }
        ride::update(world, dt);
        actmotion::update(world, dt);
        // Inferred safety: a talk the server never answers stops blocking after 10 s.
        if world.talk_lock.is_some_and(|t| t.elapsed().as_secs_f32() > 10.0) {
            world.talk_lock = None;
        }
        let in_event = event::active(world);
        target::update(world);
        // The prompt pick flashes (+0x88 |= 0x600) unless its arrow is red; so does the party cursor's.
        if let Some(t) = world.target.filter(|_| world.prompt.is_some())
            && target::range_tint(world, t) != 0xe0d0_4040
        {
            target::force_flash(world, t);
        }
        target::party_cursor_update(world);
        partymenu::update(world);
        status::update(world);
        currencies::update(world, dt * 60.0);
        merits::update(world);
        jobpoints::update(world, dt * 60.0);
        unity::update(world, dt * 60.0);
        world.cursor_keep();
        missions::update(world);
        quests::update(world);
        keyitems::update(world);
        linkshell::update(world);
        if world.party_cursor.is_some()
            && let Some((_, _, Some(id))) = target::party_hand(world)
        {
            target::force_flash(world, id);
        }
        combat::area_line(world);
        let ticks = dt * 60.0;
        // FUN_1008dfb0 / FUN_100a3560: grounded actors follow the collision floor every frame (also those that
        // arrived before the zone had loaded).
        let floor = world.ground.as_ref().filter(|_| world.in_scene() && !in_event);
        let shafts = world.lifts.shafts();
        let mut knock_ends = Vec::new();
        for (eid, e) in world.entities.iter_mut() {
            // Moved by someone else since last frame (an event script): take that as the slide and drawn spot.
            if e.pos != e.last_pos {
                e.slide = e.pos;
                e.remain = 0.0;
            }
            // FUN_100c63d0 while +0x102 is set: the push moves the drawn position directly (no easing, no walk / turn
            // motion choice); afterwards it eases back to the server's slide.
            if let Some(k) = world.knocks.get_mut(eid) {
                let (p, recover, done) = combat::knock_step(k, e.pos, ticks);
                e.pos = p;
                if e.grounded
                    && let Some(g) = floor
                    && let Some(y) = g.snap(e.pos.x, e.pos.z, e.pos.y)
                {
                    e.pos.y = y;
                }
                knock_ends.push((*eid, recover, done));
                bump::active_tick(e, ticks);
                e.last_pos = e.pos;
                continue;
            }
            // Scheduler op 0x7a: held at another actor's attach point for the move (FUN_100c63d0), no ease, no turn;
            // afterwards it eases back to its slide.
            if let Some(&(p, yaw)) = world.effects.pinned.get(eid) {
                e.pos = p;
                if let Some(y) = yaw {
                    e.yaw = y;
                }
                e.last_pos = e.pos;
                continue;
            }
            if e.dir != e.dir_seen {
                e.dir_seen = e.dir;
                e.yaw = event::rad_of(e.dir);
            }
            // FUN_1008b680: slide linearly to the goal over the remaining ticks; more than 12 units snaps.
            if (e.target - e.slide).length_squared() > 144.0 {
                e.slide = e.target;
                e.pos = e.target;
                e.yaw = event::rad_of(e.dir);
                e.remain = 0.0;
            } else if e.remain > ticks {
                e.slide += (e.target - e.slide) / e.remain * ticks;
                e.remain -= ticks;
            } else {
                e.slide = e.target;
                e.remain = 0.0;
            }
            // FUN_100c4390: the drawn position eases 0.125 a tick toward it (per tick, so any frame rate).
            let step = (e.slide - e.pos) * (1.0 - 0.875f32.powf(ticks));
            e.pos += step;
            if e.grounded
                && let Some(g) = floor
            {
                if let Some(y) = g.snap(e.pos.x, e.pos.z, e.pos.y) {
                    e.pos.y = y;
                }
                if let Some(y) = g.snap(e.target.x, e.target.z, e.target.y) {
                    e.target.y = y;
                }
                e.slide.y = e.target.y;
            }
            // Characters in a lift shaft stand at its height (FUN_100a4ed0 runs for every collision actor).
            if e.door == 0
                && e.lift.is_none()
                && !matches!(e.sub_kind, 2..=4)
                && let Some((h, _)) = lifts::ride_in(&shafts, e.last_pos, e.pos)
            {
                e.pos.y = h;
            }
            // A step under 0.01 a tick counts as standing (motion and facing).
            let flat = Vec3::new(step.x, 0.0, step.z);
            let moved = if flat.length() < 0.01 * ticks { 0.0 } else { flat.length() };
            let want = if moved > 0.0 { (-flat.z).atan2(flat.x) } else { event::rad_of(e.dir) };
            let tau = std::f32::consts::TAU;
            let diff = (want - e.yaw + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI;
            // Scheduler op 0x2f: the drawn facing stays while the effect runs (FUN_100c63d0 skips the ease).
            let rate = world.actors.get(eid).map_or(0.125, |a| turn_rate(a, in_event));
            if world.face_locks.get(eid).is_none_or(|t| Instant::now() >= *t) {
                e.yaw += diff * (1.0 - (1.0 - rate).powf(ticks));
            }
            e.ratio = crate::motion::move_ratio(e.ratio, moved, dt, e.motion.speed, e.motion.speed_base, e.motion.status);
            bump::active_tick(e, ticks);
            e.last_pos = e.pos;
        }
        // Our own push goes through the walking collision (walls, floor); the server learns it from 0x015.
        if let Some(k) = world.knocks.get_mut(&world.my_id) {
            let from = world.pos;
            let (p, recover, done) = combat::knock_step(k, from, ticks);
            let shafts = world.lifts.shafts();
            let ride = |a: Vec3, b: Vec3| lifts::ride_in(&shafts, a, b);
            world.pos = world.ground.as_ref().filter(|g| !g.is_empty()).map_or(p, |g| g.walk_player(from, p, ground::STEP_TICKS, Some(&ride)));
            knock_ends.push((world.my_id, recover, done));
        }
        for (id, recover, done) in knock_ends {
            if recover {
                combat::knock_recover(world, id);
            }
            if done {
                world.knocks.remove(&id);
            }
        }
        // FFXI_TRACE_MOVE=1: the nearest monster's drawn / goal position and motion ratio each frame (test runs).
        static TRACE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *TRACE.get_or_init(|| std::env::var_os("FFXI_TRACE_MOVE").is_some())
            && let Some((id, e)) = world.entities.iter().filter(|(_, e)| e.mob).min_by(|a, b| (a.1.pos - world.pos).length().total_cmp(&(b.1.pos - world.pos).length()))
        {
            log::info!("move {id:#x} {} t {:.3} drawn ({:.3}, {:.3}) goal ({:.3}, {:.3}) remain {:.1} ratio {:.3} yaw {:.2}", e.name, world.started.elapsed().as_secs_f32(), e.pos.x, e.pos.z, e.target.x, e.target.z, e.remain, e.ratio, e.yaw);
        }
        // A refused zone line never brings a zone-in; give up waiting.
        if world.zoning_since.is_some_and(|t| t.elapsed().as_secs_f32() > 8.0) {
            world.zoning_since = None;
        }
        // Black while zoning or loading; fade back in once we stand in the new zone. FUN_1008abe0 state 1: standing in
        // a lift shaft, the fade waits for a lift actor *(ours gives up after 10 s; retail waits)*.
        let in_shaft = world.in_scene() && !world.lifts.spawned && world.lifts.ride(world.pos, world.pos).is_some();
        if in_shaft {
            world.lift_wait.get_or_insert_with(Instant::now);
        } else {
            world.lift_wait = None;
        }
        let lift_wait = world.lift_wait.is_some_and(|t| t.elapsed().as_secs_f32() < 10.0);
        let ready = world.zoning_since.is_none() && world.in_scene() && world.renderer.is_some() && !lift_wait;
        let target = if ready && !world.unsettled { 0.0 } else { 1.0 };
        if target > world.fade {
            world.fade = (world.fade + dt / 0.4).min(1.0);
        } else if target < world.fade {
            world.fade = (world.fade - dt / 0.6).max(0.0);
        }
        if world.unsettled && world.in_scene() {
            if let Some(y) = world.ground.as_ref().and_then(|g| g.settle(world.pos.x, world.pos.z, world.pos.y)) {
                log::info!("settled from y {:.2} to {:.2}", world.pos.y, y);
                world.pos.y = y;
            }
            world.unsettled = false;
            // The camera placed at zone-in had the old (or no) ground and the server height: again behind the settled
            // player, now that the zone collision is in (ours; retail places it once, FUN_10021680).
            let yaw = world.yaw;
            camera::face(world, yaw);
        }
        let forward = Vec3::new(world.yaw.sin(), 0.0, world.yaw.cos());
        let right = forward.cross(Vec3::NEG_Y);
        let ticks = dt * 60.0;
        // Movement axes 4 / 5 (FUN_1011fe30): numpad 6 / 4 and 8 / 2 or the compact stick (W / A / S / D).
        let (mut ax, mut ay) = (self.input.axis(4), self.input.axis(5));
        // Test drives hold W; both mouse buttons run forward, as in most modern MMOs.
        if self.keys.contains(&KeyCode::KeyW) || (self.left_down && self.dragging) {
            ay = -1.0;
        }
        // FUN_10157fa0 (config 0x46, on by default): a push past 0.5 on a movement axis stands a resting character
        // up once per push: "/heal off" healing (which also ends a logout), "/sit off", "/sitchair off". The body
        // stays down until the server stands it.
        let resting = matches!(world.my_motion.status, 7 | 0x21 | 0x2f | 0x3f..=0x53);
        if (ax.abs() > 0.5 || ay.abs() > 0.5) && !in_event {
            if !self.stand_sent && resting && world.game.get(0x46) != 0 {
                let cmd = match world.my_motion.status {
                    7 | 0x21 => "/heal off",
                    0x2f => "/sit off",
                    _ => "/sitchair off",
                };
                send_chat(world, cmd.into());
            }
            self.stand_sent = true;
        } else {
            self.stand_sent = false;
        }
        // Scheduler op 0x2e: the effect of our own action holds the walk input (FUN_100a5ec0 skips FUN_100a6240).
        let move_locked = world.move_lock.is_some_and(|t| Instant::now() < t);
        if resting || move_locked {
            (ax, ay) = (0.0, 0.0);
        }
        // Camera axes 6 / 7: the arrows (with Shift in a window) or the compact I / J / K / L stick.
        let (cx, cy) = (self.input.axis(6), self.input.axis(7));
        // Keyboard type 2, free camera: Shift held faces the camera (0x9c, player +0xfb; camera keys and Q / E off),
        // and Q / E turn the player (0xaa / 0xa9, FUN_10253a60 / FUN_102539f0) while the camera axes are idle.
        let type2 = world.game.get(0x42) == 2 && !world.locked;
        let face_cam = type2 && self.input.test(0x9c, input::mode::HELD);
        let turn = if type2 && !face_cam && cx == 0.0 && cy == 0.0 && ax == 0.0 {
            self.input.test(0xaa, input::mode::HELD) as i32 as f32 - self.input.test(0xa9, input::mode::HELD) as i32 as f32
        } else {
            0.0
        };
        // Walk / run (0x45: Num/, compact Z) and autorun (0x1c: Num7, compact R): FUN_10151910 / FUN_100a4580.
        if !in_event && self.input.test(0x45, input::mode::PRESS) {
            world.walk = !world.walk;
        }
        if !in_event && self.input.test(0x1c, input::mode::PRESS) {
            world.autorun = !world.autorun;
            world.autorun_dir = Vec3::ZERO;
        }
        // Turning while autorunning steers it: the move goes forward and the turn counts as X input.
        if world.autorun && turn != 0.0 {
            (ax, ay) = (turn, -1.0);
        }
        let mut step = right * ax - forward * ay;
        // Autorun: the first move becomes the vector; X input steers it 2 degrees a tick (ticks x 0.01745 x 2); no
        // input and no vector ends it. Forced off while dead, resting or seated and in events. Locked on, the
        // keys move as usual *(inf)*.
        if world.autorun && (in_event || world.dead() || matches!(world.my_motion.status, 6 | 7 | 0x21 | 0x26..=0x2c | 0x2f | 0x30 | 0x32..=0x35 | 0x38..=0x53)) {
            world.autorun = false;
        }
        // /follow (FUN_100a6070's users): autorun points at the target each tick; the target gone, dead or without a
        // model ends it.
        if let Some(t) = world.follow {
            match world.entities.get(&t).filter(|e| !matches!(e.motion.status, 2 | 3) && world.actors.contains_key(&t)) {
                Some(e) if world.autorun => {
                    let d = Vec3::new(e.pos.x - world.pos.x, 0.0, e.pos.z - world.pos.z);
                    if d.length() > 1e-3 {
                        world.autorun_dir = d.normalize();
                    }
                }
                _ => world.autorun = false,
            }
        }
        if world.autorun && !world.locked && !move_locked {
            if world.autorun_dir == Vec3::ZERO && step != Vec3::ZERO {
                world.autorun_dir = step.normalize();
            }
            if world.autorun_dir == Vec3::ZERO {
                world.autorun = false;
            } else {
                if ax != 0.0 {
                    let d = world.autorun_dir;
                    let a = d.x.atan2(d.z) + 2.0 * ax * ticks * 0.017452778;
                    world.autorun_dir = Vec3::new(a.sin(), 0.0, a.cos());
                }
                step = world.autorun_dir;
            }
        }
        if !world.autorun {
            world.autorun_dir = Vec3::ZERO;
            // Autorun ending for any reason ends the follow: group 9 0x41 "Follow canceled.".
            if world.follow.take().is_some() {
                textcmd::log9(world, 0x41);
            }
        }
        // FUN_100a5f60: |v| > 0.9 full speed, 0.05..0.9 a third; walking caps at a third.
        let mag = if world.autorun && !world.locked { 1.0 } else { ax.hypot(ay) };
        let mut tier = if mag > 0.9 { 1.0 } else if mag >= 0.05 { 1.0 / 3.0 } else { 0.0 };
        if world.walk {
            tier = f32::min(tier, 1.0 / 3.0);
        }
        // FUN_100964f0: no movement during a synthesis (status 0x2c).
        if world.my_motion.status == 0x2c {
            tier = 0.0;
        }
        if tier == 0.0 {
            step = Vec3::ZERO;
        }
        world.move_tier = tier;
        // Q / E: player yaw += ticks x 0.01745 x 1.57075; the camera follows the turn (FUN_1001d4c0).
        let turn_delta = if turn != 0.0 && !world.autorun && !in_event && world.unlock_ticks <= 0.0 { turn * ticks * 0.017452778 * 1.57075 } else { 0.0 };
        world.yaw += turn_delta;
        let turn_keys = if face_cam { 0.0 } else { cx };
        // Mouse/Cam.: 0x8b / 0x8c invert the camera axes, 0x8d / 0x8e in first person.
        let first = world.camst.first;
        let (i6, i7) = [(0x8b, 0x8c), (0x8d, 0x8e)][first as usize];
        let (i6, i7) = (if world.game.get(i6) == 1 { -1.0 } else { 1.0 }, if world.game.get(i7) == 1 { -1.0 } else { 1.0 });
        let still = face_cam || in_event;
        let mut cam_in = camera::Inputs {
            v6: if still { 0.0 } else { cx * i6 },
            v7: if still { 0.0 } else { cy * i7 },
            turn: turn_delta,
            face_cam,
            zoom_in: self.input.test(0x4f, input::mode::HELD),
            zoom_out: self.input.test(0x50, input::mode::HELD),
            home: self.input.test(0x51, input::mode::HELD),
            wheel: std::mem::take(&mut self.wheel),
            ..Default::default()
        };
        // First / third person (0x10: Num5, compact V; FUN_100a4580): SE 0014. The dead are third person.
        if !in_event && !world.dead() && self.input.test(0x10, input::mode::PRESS) {
            camera::toggle_first(world);
            world.sounds.push(14);
        }
        if world.dead() && world.camst.first {
            camera::toggle_first(world);
        }
        // Screen edge panning (0x8f = 0, FUN_10120fc0): the pointer at the window's left / right edge turns the
        // camera *(inf: margin and rate)*.
        let edge = match (world.game.get(0x8f), self.last_cursor, self.gfx.as_ref()) {
            (0, Some((cx, _)), Some(g)) if self.cursor_in && !world.locked => {
                let w = g.config.width as f64;
                if cx <= 2.0 { -1.0 } else if cx >= w - 3.0 { 1.0 } else { 0.0 }
            }
            _ => 0.0,
        };
        if edge != 0.0 && !in_event && world.unlock_ticks <= 0.0 {
            cam_in.edge = edge as f32 * 1.5 * dt;
        }
        // Locked: the keys move relative to the target (FUN_100a6210) and the camera follows FUN_1001e030.
        let lock_target = world.target.filter(|_| world.locked).and_then(|t| world.entities.get(&t)).map(|e| e.pos);
        if let Some(tp) = lock_target {
            // FUN_1001ee60 locked: axis 6 turns at the free rate (scaled 6 / distance in step_camera), axis 7 raises
            // and lowers the eye at the free rate, unscaled.
            let (dx, dy) = std::mem::take(&mut world.cam_turn);
            let ticks = dt * 60.0;
            target::step_camera(world, ticks, dx + turn_keys * i6 * camera::YAW_RATE * ticks, dy + cy * i7 * camera::RISE_RATE * ticks);
            let to = tp - world.pos;
            if to.x.abs() + to.z.abs() > 1e-3 {
                world.yaw = to.x.atan2(to.z);
            }
        } else {
            world.cam_turn = (0.0, 0.0);
        }
        let face = Vec3::new(world.yaw.sin(), 0.0, world.yaw.cos());
        // The KO'd and the resting (status 33, also the logout countdown) can look around but not move;
        // in an event the script moves us (FUN_100954f0 runs only without one).
        if world.dead() || world.my_motion.status == 33 || in_event {
            step = Vec3::ZERO;
        }
        let stepping = step != Vec3::ZERO;
        let moving = stepping;
        // Whole retail frames of FUN_100a48d0 / FUN_100a3560; a position set elsewhere starts over from it.
        if world.pos != world.step_to {
            (world.step_hist, world.step_to, world.step_ticks) = ([world.pos; 4], world.pos, 0.0);
        }
        world.step_ticks += dt * 60.0;
        // Speed byte x0.1 = units per second (5.0 for a normal run).
        let speed = if world.my_motion.speed > 0 { world.my_motion.speed as f32 * 0.1 } else { RUN_SPEED };
        let falls = !in_event && !world.unsettled && world.in_scene();
        let shafts = world.lifts.shafts();
        let ride = |a: Vec3, b: Vec3| lifts::ride_in(&shafts, a, b);
        let mut steps = 0;
        while world.step_ticks >= ground::STEP_TICKS {
            world.step_ticks -= ground::STEP_TICKS;
            steps += 1;
            // After a hitch, drop the backlog.
            if steps > 8 {
                world.step_ticks = 0.0;
                break;
            }
            let from = world.pos;
            let mut next = from;
            if stepping {
                let s = step.normalize();
                let d = speed * tier * ground::STEP_TICKS / 60.0;
                next = match lock_target {
                    Some(tp) => target::locked_step(from, tp, s.dot(forward), s.dot(right), d),
                    None => from + s * d,
                };
            }
            // FUN_100a8770 at the end of the move input (also with no input): a character in the way holds the step
            // (the run motion and facing stay).
            if !in_event && bump::blocks(world, from, next - from, ground::STEP_TICKS) {
                next = from;
            }
            let g = world.ground.as_ref().filter(|g| !g.is_empty());
            if next != from {
                world.pos = g.map_or(next, |g| g.walk_player(from, next, ground::STEP_TICKS, Some(&ride)));
            } else if falls && let Some(g) = g {
                // A fall goes on without input.
                world.pos = g.walk_player(from, from, ground::STEP_TICKS, Some(&ride));
            } else if let Some((h, _)) = ride(from, from) {
                // FUN_100a4ed0 runs in events and standing still too.
                world.pos.y = h;
            }
            world.step_hist = [world.step_hist[1], world.step_hist[2], world.step_hist[3], world.pos];
        }
        world.step_to = world.pos;
        world.draw_pos = drawn_between_steps(&world.step_hist, world.step_ticks / ground::STEP_TICKS);
        // On a lift the feet are drawn at its height now (the model is too), and the camera eye rides along
        // (FUN_1001ee60: eye.y += the player's y change, the hold set) by the drawn height, every frame as the lift
        // and the look point move: the stepped y would jolt the eye at 30 Hz against them.
        let rode = ride(world.pos, world.pos);
        world.riding = rode.is_some();
        if let Some((h, _)) = rode {
            world.draw_pos.y = h;
            camera::ride(world, h - world.ride_y);
        }
        world.ride_y = world.draw_pos.y;
        if *TRACE.get_or_init(|| std::env::var_os("FFXI_TRACE_MOVE").is_some()) && (stepping || world.riding) {
            let c = Self::camera(world);
            log::info!("me t {:.4} steps {steps} y {:.4} drawn ({:.3}, {:.4}, {:.3}) eye ({:.3}, {:.4}, {:.3}) look ({:.3}, {:.4}, {:.3})", world.started.elapsed().as_secs_f32(), world.pos.y, world.draw_pos.x, world.draw_pos.y, world.draw_pos.z, c.eye.x, c.eye.y, c.eye.z, c.target.x, c.target.y, c.target.z);
        }
        camera::step(world, dt, &cam_in);
        let in_event = event::active(world);
        world.effects.in_event = in_event;
        camera::shake_step(world, dt);
        if *TRACE.get_or_init(|| std::env::var_os("FFXI_TRACE_MOVE").is_some()) && (cx != 0.0 || cy != 0.0 || (world.started.elapsed().as_secs_f32() * 2.0).fract() < dt * 2.0) {
            log::info!("cam t {:.3} eye {:.3} look {:.3} drawn {:.3} unlock {:.1} v6 {cx:.2} v7 {cy:.2} first {}", world.started.elapsed().as_secs_f32(), world.cam.eye, world.cam.look, world.draw_pos, world.unlock_ticks, world.camst.first);
        }
        // Touching a zone line asks the server to move us; wait for its answer before asking again.
        if stepping && world.zoning_since.is_none_or(|t| t.elapsed().as_secs_f32() > 5.0) {
            if let Some(line) = world.zone_lines.iter().find(|r| r.contains(world.pos.to_array())) {
                world.zoning_since = Some(Instant::now());
                log::info!("zone line {}", line.name);
                let _ = world.net.commands.send(NetCommand::ZoneLine { tag: line.tag(), pos: world.pos.to_array() });
            }
        }
        if let Ok(mut me) = world.net.me.lock() {
            me.pos = world.pos.to_array();
            // Standing still keeps the facing; only the camera turns. Locked, we face the target.
            if lock_target.is_some() || face_cam || (turn != 0.0 && !stepping) {
                me.dir = dir_from_forward(face);
            } else if stepping {
                me.dir = dir_from_forward(step.normalize());
            }
            me.moving = moving;
        }
        world.moving = moving;
    }

    /// The frame's key state, then the key functions that run outside any window.
    fn key_functions(&mut self, dt: f32) {
        let mut f = input::Frame { ticks: dt * 60.0, ..Default::default() };
        if let Phase::World(w) = &self.phase {
            self.input.assign(&w.game);
            self.input.context = if w.map.is_some() || w.cnq.is_some() { 0x26 } else { 0xffff };
            f.typing = text_field_open(w);
            f.keyboard = w.game.get(0x42);
            // A window has the keys: arrows go to it, not to the camera (keys_spec "window" state). The log counts only
            // while focused: Escape or another window unfocuses it and leaves the 0x78 state at 1.
            f.window = world_window_open(w) || w.palette.0 != 0 || w.log_active || w.log_focus == 2 || event::choice(w).is_some() || event::input(w).is_some();
            f.assigning = w.config.as_ref().and_then(|c| c.stack.last()).is_some_and(|t| t.name == "k1assign" || t.name == "k2assign");
        }
        self.input.begin_frame(f);
        let Phase::World(world) = &mut self.phase else { return };
        // The map window's keys and animations (FUN_101ef0a0); it goes once its fade-out ends, reopening the main menu
        // it came from (LAB_101faff0).
        let k = self.ui_k;
        let drag = self.press_at.zip(self.last_cursor).filter(|_| self.left_down).map(|(a, b)| ((b.0 - a.0) as f32 * k.0, (b.1 - a.1) as f32 * k.1));
        if world.cnq.is_some() && !region::update(world, &mut self.input, dt * 60.0) {
            world.cnq = None;
        }
        if let Some(m) = world.map.as_mut()
            && !m.update(&mut self.input, dt * 60.0, drag)
        {
            let (back, reopen, mode) = (m.return_menu, m.reopen, m.mode);
            world.map = None;
            world.scan.party_busy = false;
            match reopen {
                // LAB_101faff0 -> FUN_101f79e0(6, zone, map): the preview reopens there, without the arrow.
                Some((zone, key)) if mode == map::Mode::Preview => world.map_cmds.push(map::Cmd::Preview { zone, key, arrow: false, point_y: 0.0 }),
                Some((zone, key)) => world.map_cmds.push(map::Cmd::Other { zone, key, return_menu: back }),
                None if back.is_some() => world.menu = back,
                None => {}
            }
        }
        // Key function 0x18 also turns an RoE description's page (FUN_102326b0).
        if self.input.test(0x18, input::mode::PRESS) {
            roe::flip_page(world);
        }
        // FUN_1013c220: key function 0x18 (Num- / -) turns the item info window's page; past the last, back to 0.
        if self.input.test(0x18, input::mode::PRESS) && (world.info_win.is_some() || items::info_source(world).is_some()) {
            let mut p = world.info_page.get();
            p.0 = if p.0 + 1 >= p.2.min(3) { 0 } else { p.0 + 1 };
            world.info_page.set(p);
        }
        items::compare_key(world, self.input.test(0x76, input::mode::HELD) || self.input.test(0x89, input::mode::HELD));
        if self.input.test(0x11, input::mode::PRESS) && world.bluset.is_some() {
            bluset::num_plus(world);
        } else if self.input.test(0x11, input::mode::PRESS) && (world.magic.is_some() || world.ability.is_some()) {
            magic::num_plus(world);
        }
        if self.input.test(0x11, input::mode::PRESS) && matches!(world.win, Some(items::Win::Items { store: Some(_), .. })) {
            items::store_num_plus(world);
        }
        if self.input.test(0x11, input::mode::PRESS) && matches!(world.win, Some(items::Win::Equip(_))) {
            equipwin::num_plus(world);
        }
        if self.input.test(0x11, input::mode::PRESS) && synth::active(world) {
            synth::num_plus(world);
        }
        if self.input.test(0x11, input::mode::PRESS) && treasure::active(world) {
            treasure::num_plus(world);
        }
        if self.input.test(0x11, input::mode::PRESS) && world.macroedit.is_some() {
            macroedit::num_plus(world);
        }
        // mcrbedit's set mode: Ctrl (0x76) or Alt (0x77) held.
        let held = self.input.test(0x76, input::mode::HELD) || self.input.test(0x77, input::mode::HELD);
        macroedit::update(world, dt * 60.0, held);
        // The search results' list message 8 (FUN_1020a580): the job cells' other view, no sound.
        if self.input.test(0x11, input::mode::PRESS) && world.arealist.is_some() {
            if let Some(gu) = self.game_ui.as_ref() {
                arealist::num_plus(world, gu);
            }
        } else if self.input.test(0x11, input::mode::PRESS) && world.search.as_ref().is_some_and(|s| s.option.is_none()) && !searchmenu::refining(world) {
            search::toggle_view(world);
        } else if self.input.test(0x11, input::mode::PRESS) && world.linklist.is_some() {
            linklist::toggle_view(world);
        }
        // Key function 0x11 (Num+): the item list's focus to itmsort2 and back (list message 8, FUN_101f3740), SE 0013.
        if self.input.test(0x11, input::mode::PRESS)
            && let Some(items::Win::Items { direct: false, sub: None, manual: None, store: None, sort, .. }) = &mut world.win
        {
            *sort = match sort {
                Some(_) => None,
                None => Some(items::SortWin { cursor: 1, ask: None }),
            };
            world.sounds.push(crate::audio::Ui::Close as u32);
        }
        // Numpad * / compact H: 0x44 (/heal) and 0x30 (lock-on) read the same press; FUN_101516d0 picks.
        if self.input.test(0x44, input::mode::PRESS) {
            target::numpad_star(world);
        }
        // Numpad + / Ctrl or Alt + F / compact F, mode 0 (repeats): FUN_10151550.
        if self.input.test(0x78, input::mode::REPEAT) {
            log_cycle(world);
        }
        // Macros (FUN_101a2e80): the runner steps first, one line a 2-tick frame, through the text-command path; then,
        // when the gate allows (field, UI shown, no event, alive, no window), Ctrl / Alt + digit runs a macro and
        // Shift + Ctrl / Alt + digit picks a set. The palettes are not drawn yet.
        let sub_up = target::sub_up(world);
        if let Some(line) = world.macros.as_mut().filter(|_| !sub_up).and_then(|m| m.step(dt * 60.0)) {
            textcmd::run(world, &crate::dat::font::decode_sjis(&line), textcmd::Mode::Macro);
        }
        // FUN_101a33e0: no window has the focus, or the one that has it is the action menu (playermo / mp_pmode) or a
        // palette; the action menu stays up while engaged.
        let window = window_over_actions(world) || world.settings.is_some() || world.full_log.is_some();
        let gate = world.in_scene() && !world.ui_hidden && !event::active(world) && !world.dead() && !window && world.macros.is_some();
        // FUN_101a2e80 step 3: Ctrl (0x76) / Alt (0x77) held opens palette 1 / 2 at the next whole second of the
        // clock; released, it closes. Ctrl wins.
        let held = if self.input.test(0x76, input::mode::HELD) { 1 } else if self.input.test(0x77, input::mode::HELD) { 2 } else { 0 };
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let pal = &mut world.palette;
        if held == 0 || !gate {
            (pal.0, pal.1, pal.2) = (0, 0, 0);
        } else if pal.0 != held {
            if pal.1 != held {
                (pal.1, pal.2) = (held, secs);
            } else if secs != pal.2 {
                pal.0 = held;
            }
        }
        if gate {
            const SETS: [usize; 10] = [0x59, 0x5a, 0x5b, 0x5c, 0x5d, 0x5e, 0x5f, 0x60, 0x61, 0x58];
            const CTRL: [usize; 10] = [0x6d, 0x6e, 0x6f, 0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x6c];
            const ALT: [usize; 10] = [0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x6b, 0x62];
            if let Some(set) = SETS.iter().position(|&k| self.input.test(k, input::mode::PRESS)) {
                if let Some(m) = world.macros.as_mut() {
                    let book = m.book;
                    m.switch(book, set);
                }
            } else if let Some(n) = CTRL.iter().position(|&k| self.input.test(k, input::mode::PRESS)).or_else(|| ALT.iter().position(|&k| self.input.test(k, input::mode::PRESS)).map(|n| n + 10)) {
                if let Some(m) = world.macros.as_mut() {
                    m.start(n);
                }
            }
        }
        // FUN_100e9550: a listed member with no name asks the roster again (c2s 0x076), once per table.
        if world.party.nameless() && !world.party_asked {
            world.party_asked = true;
            let _ = world.net.commands.send(NetCommand::GroupListReq);
        }
        // Key function 0x56 (Scroll Lock, FUN_10160710): hide / show the UI, also while typing.
        if self.input.test(0x56, input::mode::PRESS) && !event::active(world) {
            world.ui_hidden = !world.ui_hidden;
        }
        // Key function 0x55 (Print Screen, FUN_101d0ab0): a screenshot, only with the UI hidden, in the field with the
        // map closed; the shutter SE "0016". Saved as FUN_101d0ea0 names it: the name's first three letters,
        // yymmddhhmmss and a letter for shots in the same second, in the game folder's screenshots (PNG here, retail
        // JPG).
        if self.input.test(0x55, input::mode::PRESS) && world.ui_hidden && world.map.is_none() && world.in_scene() {
            let t = jiff::Zoned::now();
            let sec = t.second();
            world.shot_count = if world.shot_count.1 == sec { (world.shot_count.0.wrapping_add(1), sec) } else { (0, sec) };
            let initials: String = world.name.chars().take(3).collect();
            let name = format!("{initials}{:02}{:02}{:02}{:02}{:02}{:02}{}.png", t.year() % 100, t.month(), t.day(), t.hour(), t.minute(), sec, (b'a' + world.shot_count.0 % 10) as char);
            world.shot_request = Some(self.cfg.ffxi_dir().join("screenshots").join(name));
            world.sounds.push(16);
        }
        // Key function 0x13 (Pause, FUN_101609f0): the log-out prompt, in the field, not engaged, with the UI shown.
        if self.input.test(0x13, input::mode::PRESS) && !event::active(world) && !world.ui_hidden && world.my_motion.status != 1 && world.confirm.is_none() && world.ko_offer != 2 {
            world.confirm = Some((Confirm::Logout, false));
        }
        // Target keys (FUN_101587f0): Tab / Num0 and with Shift (0x84 / 0x97), F8 / Ctrl+F8 / Alt+F8 / F9 / F10; not
        // while the K.O. window or the raise prompt has the focus (FUN_101585f0).
        if !event::active(world) && !world.ko_open && world.ko_offer != 2 {
            let view = Self::camera(world).view();
            if self.input.test(0x84, input::mode::PRESS) {
                target::cycle(world, view, true);
            } else if self.input.test(0x97, input::mode::PRESS) {
                target::cycle(world, view, false);
            } else if self.input.test(0x4d, input::mode::PRESS) {
                target::nearest_kind(world, 0x10, false, false);
            } else if self.input.test(0xab, input::mode::PRESS) {
                target::nearest_kind(world, 0x10, true, false);
            } else if self.input.test(0x52, input::mode::PRESS) || self.input.test(0xac, input::mode::PRESS) {
                target::nearest_kind(world, 0x12, false, false);
            } else if self.input.test(0x4e, input::mode::PRESS) {
                target::nearest_kind(world, 0, false, true);
            } else if self.input.test(0x46, input::mode::PRESS) || self.input.test(0x4c, input::mode::PRESS) {
                // F1 / F7: ourselves (in a prompt only when it takes us).
                if world.prompt.is_some() {
                    target::sub_self(world);
                } else {
                    target::pick(world, Some(world.my_id));
                }
            } else if let Some(slot) = [(0x47, 1), (0x48, 2), (0x49, 3), (0x4a, 4), (0x4b, 5), (0x53, 6), (0x54, 12)].into_iter().find(|(k, _)| self.input.test(*k, input::mode::PRESS)).map(|(_, s)| s) {
                // F2..F6 party slots 1..5, F11 / F12 the other parties' first members (FUN_102033c0 / FUN_102034a0).
                if world.prompt.is_some() {
                    target::sub_slot(world, slot);
                } else if let Some(t) = target::slot_target(world, slot) {
                    target::pick(world, Some(t));
                }
            }
        }
    }

    /// FUN_10125360's kind for this frame: a drag shows the arrow (both buttons the move arrow pointing ahead,
    /// otherwise the camera arrow along the drag), an actor under the pointer in the field the hit cursor.
    /// Over an enabled menu item, kind 1. Not done: 3 / 6 (scroll thumb, autoscroll).
    fn mouse_kind(&mut self, size: [u32; 2]) {
        // FUN_10125360: with the UI hidden the cursor is hidden too.
        if let Phase::World(w) = &self.phase
            && w.ui_hidden
        {
            self.mouse.visible = false;
        }
        let at = self.last_cursor;
        let moved = self.left_down && self.press_at.zip(at).is_some_and(|(a, b)| (a.0 - b.0).abs() + (a.1 - b.1).abs() >= 5.0);
        self.mouse.kind = mousecur::NORMAL;
        if self.left_down && self.dragging {
            self.mouse.kind = mousecur::MOVE_DRAG;
            self.mouse.angle = -std::f32::consts::PI;
            return;
        }
        if self.dragging || moved {
            if let (Some(from), Some(to)) = (self.press_at.or(self.drag_from), at) {
                self.mouse.camera_drag((from.0 as f32, from.1 as f32), (to.0 as f32, to.1 as f32));
            }
            return;
        }
        if self.mouse.visible && self.game_ui.as_ref().is_some_and(|g| g.hover.get()) {
            self.mouse.kind = mousecur::OVER_ITEM;
            return;
        }
        let Phase::World(world) = &mut self.phase else { return };
        // FUN_10126270: the hit cursor only while no window has the focus (a sub-target prompt picks anyway).
        let window = world_window_open(world) && world.prompt.is_none();
        if let Some(p) = at.filter(|_| !window && self.mouse.visible) {
            let aspect = cfgapply::aspect(size, self.cfg.retail_aspect, world.game.get(0x43));
            let (picked, rect) = Self::under_pointer(world, p, size, aspect);
            world.pick_rect = rect;
            if picked.is_some() {
                self.mouse.kind = mousecur::OVER_ACTOR;
            }
        } else {
            world.pick_rect.clear();
        }
    }

    /// A mouse button (from the window or a test script): presses, drags, and the world click on releases
    /// (FUN_101585f0: key functions 0x8b / 0x8c -> results 0x16 / 0x17); a drag is 65 UI px^2 from the press or 25
    /// ticks held (FUN_10125040).
    fn mouse_button(&mut self, button: MouseButton, pressed: bool, consumed: bool) {
        self.mouse.visible = true;
        if pressed && button == MouseButton::Right {
            self.drag_from = self.last_cursor;
        }
        let release = |from: Option<((f64, f64), Instant)>, at: Option<(f64, f64)>, k: (f32, f32)| -> Option<((f64, f64), bool)> {
            let ((a, t), b) = (from?, at?);
            let (dx, dy) = ((b.0 - a.0) as f32 * k.0, (b.1 - a.1) as f32 * k.1);
            Some((b, dx * dx + dy * dy >= 65.0 || t.elapsed().as_secs_f32() * 60.0 >= 25.0))
        };
        let size = self.window_size;
        let (k, retail_aspect) = (self.ui_k, self.cfg.retail_aspect);
        let hud = &self.hud_blockers;
        let world_click = |phase: &mut Phase, b: (f64, f64), left: bool, drag: bool| -> bool {
            let on_hud = hud.iter().any(|r| inside(r, (b.0 as f32 * k.0, b.1 as f32 * k.1)));
            let Phase::World(world) = phase else { return false };
            let gated = consumed || event::active(world) || world.typing || world.ui_hidden || (world_window_open(world) && world.prompt.is_none());
            if on_hud || gated {
                return false;
            }
            let aspect = cfgapply::aspect(size, retail_aspect, world.game.get(0x43));
            let picked = if left { Self::under_pointer(world, b, size, aspect).0 } else { None };
            target::click(world, picked, left, drag)
        };
        match button {
            MouseButton::Left => {
                if pressed {
                    self.press_at = self.last_cursor;
                    self.press_time = Some(Instant::now());
                } else if let Some((b, drag)) = release(self.press_at.take().zip(self.press_time.take()), self.last_cursor, self.ui_k) {
                    self.swallow_click |= world_click(&mut self.phase, b, true, drag);
                }
                self.left_down = pressed;
            }
            MouseButton::Right => {
                if pressed {
                    self.right_press = self.last_cursor.map(|c| (c, Instant::now()));
                } else if let Some((b, drag)) = release(self.right_press.take(), self.last_cursor, self.ui_k) {
                    self.swallow_click |= world_click(&mut self.phase, b, false, drag);
                }
                self.dragging = pressed;
            }
            MouseButton::Middle => {
                // WM_MBUTTONDOWN turns an RoE description's page (FUN_102326b0).
                if pressed && let Phase::World(w) = &mut self.phase {
                    roe::flip_page(w);
                }
                self.middle_down = pressed;
            }
            _ => {}
        }
    }

    fn camera(world: &World) -> Camera {
        let mut c = event::camera(world).unwrap_or_else(|| camera::view(world));
        world.camst.shake.apply(&mut c);
        c
    }

    /// FUN_10083750: the actor under a cursor position and the actors whose screen rect holds it. Candidates
    /// (FUN_10083430 with self) near-first by the box's farthest corner; the first whose rect (vtable +0x3d0) holds
    /// the pointer and whose fine test (+0x3d4) hits wins.
    fn under_pointer(world: &World, cursor: (f64, f64), size: [u32; 2], aspect: f32) -> (Option<u32>, HashSet<u32>) {
        let cam = Self::camera(world);
        let (view, proj) = (cam.view(), cam.proj(aspect));
        let vp = proj * view;
        let ndc = glam::Vec2::new(2.0 * cursor.0 as f32 / size[0].max(1) as f32 - 1.0, 1.0 - 2.0 * cursor.1 as f32 / size[1].max(1) as f32);
        let hidden = event::hidden(world);
        let my_dir = world.net.me.lock().map(|m| m.dir).unwrap_or(0);
        let rect = |corners: &[Vec3], m: glam::Mat4| -> Option<(glam::Vec2, glam::Vec2, f32)> {
            let (mut lo, mut hi, mut far) = (glam::Vec2::splat(f32::MAX), glam::Vec2::splat(f32::MIN), 0.0f32);
            for c in corners {
                let p = m * c.extend(1.0);
                if p.w <= 0.0 {
                    return None;
                }
                let q = glam::Vec2::new(p.x / p.w, p.y / p.w);
                (lo, hi, far) = (lo.min(q), hi.max(q), far.max(p.w));
            }
            Some((lo, hi, far))
        };
        let inside = |lo: glam::Vec2, hi: glam::Vec2| ndc.x >= lo.x && ndc.x <= hi.x && ndc.y >= lo.y && ndc.y <= hi.y;
        let mut hits: Vec<(f32, u32, bool)> = Vec::new();
        for (id, a) in &world.actors {
            let Some(ch) = &a.character else { continue };
            if !target::mouse_candidate(world, &hidden, *id) || a.alpha / actmotion::OPAQUE < 0.01 {
                continue;
            }
            let Some(place) = actor_place(world, *id, my_dir) else { continue };
            // FUN_100cfc10: the status box as a plain box (x b2 / b3), b0 == b1 widened, then b2 == b0 (sic); no scale.
            let Some(&[mut b0, mut b1, mut b2, mut b3, b4, b5]) = ch.skeleton.boxes.get(box_index(place.status)) else { continue };
            if b0 == b1 {
                (b0, b1) = (b0 - 1.0, b1 + 1.0);
            }
            if b2 == b0 {
                (b2, b3) = (b2 - 1.0, b3 + 1.0);
            }
            let mut corners = Vec::with_capacity(8);
            for x in [b2, b3] {
                for y in [b0, b1] {
                    for z in [b4, b5] {
                        corners.push(Vec3::new(x, y, z));
                    }
                }
            }
            if let Some((lo, hi, far)) = rect(&corners, vp * actor_matrix(place.pos, place.forward))
                && inside(lo, hi)
            {
                hits.push((far, *id, true));
            }
        }
        // Doors without a region box (FUN_100ad320): a 0.5-tall box at the door, always a hit inside it.
        for (id, e) in world.entities.iter().filter(|(id, e)| (e.door != 0 || **id == myroom::DOOR_ID) && target::mouse_candidate(world, &hidden, **id)) {
            let (Some(a), Some(b)) = (rect(&[e.pos], vp), rect(&[e.pos + Vec3::new(0.0, -0.5, 0.0)], vp)) else { continue };
            let h = (b.0.y - a.0.y).abs() * 0.5;
            let (lo, hi) = (glam::Vec2::new(a.0.x - h, a.0.y.min(b.0.y)), glam::Vec2::new(a.0.x + h, a.0.y.max(b.0.y)));
            if inside(lo, hi) {
                hits.push((a.2, *id, false));
            }
        }
        hits.sort_by(|x, y| x.0.total_cmp(&y.0));
        let in_rect: HashSet<u32> = hits.iter().map(|h| h.1).collect();
        let picked = hits.iter().find(|&&(_, id, skinned)| !skinned || Self::fine_hit(world, id, view, proj, ndc)).map(|h| h.1);
        (picked, in_rect)
    }

    /// FUN_100cff40: circles at each geometry bone, its parent and the midpoint (radius 0.4 x the segment on screen),
    /// else around attach points 1 and 3 (world radius from points 13 / 15). From the read-back points; before any
    /// came back the rect alone decides (inferred).
    fn fine_hit(world: &World, id: u32, view: glam::Mat4, proj: glam::Mat4, ndc: glam::Vec2) -> bool {
        let Some(a) = world.actors.get(&id) else { return false };
        let Some(ch) = &a.character else { return false };
        if ch.skeleton.bones.len() <= 2 {
            return true;
        }
        let Some(pts) = world.pick_points.get(&id) else { return true };
        let my_dir = world.net.me.lock().map(|m| m.dir).unwrap_or(0);
        let Some(root) = actor_place(world, id, my_dir).map(|p| p.pos) else { return false };
        let vp = proj * view;
        let screen = |p: Vec3| {
            let c = vp * (root + p).extend(1.0);
            (c.w > 1e-3).then(|| glam::Vec2::new(c.x / c.w, c.y / c.w))
        };
        for &b in a.pick_bones.iter().flatten() {
            let parent = ch.skeleton.bones[b as usize].parent as u16;
            let (Some(pa), Some(pb)) = (pts.get(&b).and_then(|p| screen(*p)), pts.get(&parent).and_then(|p| screen(*p))) else { continue };
            let r = 0.4 * (pa - pb).length();
            if r >= 0.01 && [pa, pb, (pa + pb) * 0.5].iter().any(|c| (ndc - *c).length() < r) {
                return true;
            }
        }
        let horiz = |k: u16| pts.get(&(0x8000 | k)).map_or(0.0, |p| glam::Vec2::new(p.x, p.z).length());
        let (l13, l15) = (horiz(13), horiz(15));
        let big_r = 0.75 * l13.min(l15) + 0.25 * l13.max(l15);
        [1u16, 3].iter().any(|&k| {
            let Some(p) = pts.get(&(0x8000 | k)) else { return false };
            let v = view.transform_point3(root + *p);
            let clip = |q: Vec3| {
                let c = proj * q.extend(1.0);
                glam::Vec2::new(c.x, c.y) / c.w.max(0.001)
            };
            let (c, e) = (clip(v), clip(v + Vec3::new(big_r, 0.0, 0.0)));
            let r = e.x - c.x;
            (ndc - c).length_squared() < r * r
        })
    }

    fn ui(&mut self, ctx: &egui::Context, window: [u32; 2]) {
        let mut action: Option<Box<dyn FnOnce(&mut App)>> = None;
        self.ui_drawn = false;
        // Config-tool 0037 / 0038: the UI is laid out at its own size and stretched to the window.
        let size = self.cfg.ui_size(window);
        self.ui_k = (size[0] as f32 / window[0].max(1) as f32, size[1] as f32 / window[1].max(1) as f32);
        let k = self.ui_k;
        let screen_size = size;
        let ppp = ctx.pixels_per_point();
        // The stacked-window pointer follow, only with the drawn (software) cursor.
        let ptr_dy = match &self.phase {
            Phase::World(w) if self.cfg.tool("0021") == 0 => w.ptr_dy,
            _ => 0.0,
        };
        let swallow = ctx.input(|i| i.pointer.primary_released()) && std::mem::take(&mut self.swallow_click);
        let notches = self.list_wheel.trunc();
        self.list_wheel -= notches;
        let input = ctx.input(|i| UiInput {
            wheel: notches as i32,
            pointer: i.pointer.hover_pos().map(|p| (p.x * ppp * k.0, p.y * ppp * k.1 + ptr_dy)),
            moved: i.pointer.delta() != egui::Vec2::ZERO,
            clicked: i.pointer.primary_clicked() && !swallow,
            pressed: i.pointer.primary_pressed(),
            down: i.pointer.primary_down(),
            released: i.pointer.primary_released() && !swallow,
            right_clicked: i.pointer.secondary_clicked(),
            events: i.events.clone(),
        });
        if input.moved
            && let Some(gu) = &self.game_ui
        {
            gu.hover.set(false);
        }
        let busy = self.busy;
        let status = self.status.clone();
        let (mut apply, mut save) = (false, false);
        let mut settings_new = false;
        let mut lobby_apply = LobbyApply::default();
        // The game UI is the only UI: until its data loads nothing draws, and if it fails one plain error shows.
        if let Some(fr) = self.first_run.as_mut() {
            match fr.ui(ctx) {
                firstrun::Action::Quit => self.exit_requested = true,
                firstrun::Action::Ready => {
                    // The folder may have been changed in the window: the config keeps it.
                    self.cfg.game_dir = fr.game_dir().to_path_buf();
                    self.first_run = None;
                    // Audio opened on the empty folder (and the title music failed): open it on the new files.
                    self.audio = None;
                    self.audio_tried = false;
                    self.index = None;
                    self.keys_table = None;
                    self.game_ui_state = LoadState::Idle;
                    self.flyover_state = LoadState::Idle;
                    self.cfg.installed = true;
                    // From the fixed first-start window to the configured one.
                    self.window_dirty = true;
                    if let Err(e) = self.cfg.save() {
                        log::warn!("saving the config: {e:#}");
                    }
                    self.updates = Some(updater::start(self.cfg.ffxi_dir()));
                }
                firstrun::Action::Begin(dir) => {
                    self.cfg.game_dir = dir;
                    if let Err(e) = self.cfg.save() {
                        log::warn!("saving the config: {e:#}");
                    }
                }
                firstrun::Action::None => {}
            }
            return;
        }
        if self.game_ui.is_none() {
            if self.game_ui_state == LoadState::Failed {
                egui::Area::new(egui::Id::unique("error")).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(format!("The game data could not be loaded.\n{status}"));
                });
            }
            return;
        }
        // Screen changes and leaving the lobby wait for the fade-out (LobbyFade); input meanwhile is dropped.
        let before = if let Phase::Characters { screen, .. } = &self.phase { Some(*screen) } else { None };
        let fading = self.lobby_fade.busy();
        let mut fade_action = false;
        match &mut self.phase {
            Phase::Login { password, otp, focus } => {
                self.title_press = None;
                if let Some(lu) = self.game_ui.as_mut() {
                    self.ui_drawn = true;
                    let fields = [&mut self.cfg.server, &mut self.cfg.username, &mut *password, &mut *otp];
                    // While an error shows, the fields take no input (the OK window has it).
                    let none = UiInput { pointer: None, moved: false, clicked: false, pressed: false, down: false, released: false, right_clicked: false, wheel: 0, events: Vec::new() };
                    let shown = if self.login_error.is_some() { &none } else { &input };
                    let mut sounds = Vec::new();
                    let chosen = login_window(lu, shown, size, fields, focus, busy, &status, &self.cfg.client_version, &mut sounds);
                    if let Some(a) = self.audio.as_mut() {
                        for s in sounds {
                            a.ui(s);
                        }
                    }
                    // Lobby and login errors in retail's OK window (menu ok, FUN_102319a0): "Error code: FFXI-%04d" and
                    // the table's message for a known code (FUN_10017310), else the text as it came.
                    if let Some(e) = self.login_error.clone() {
                        let code = e.rsplit_once("(FFXI-").and_then(|(_, c)| c.trim_end_matches(')').parse::<u32>().ok());
                        let text = match code {
                            Some(c) => {
                                let t = lu.error_text(c);
                                if t.contains('\n') { t } else { format!("{t}\n{}", e.rsplit_once(" (FFXI-").map_or(e.as_str(), |x| x.0)) }
                            }
                            None => e.clone(),
                        };
                        let (sw, sh) = (size[0] as f32, size[1] as f32);
                        let ok = lu.ok_window(&text, sw, sh);
                        let key = input.events.iter().any(|ev| matches!(ev, egui::Event::Key { key: egui::Key::Enter | egui::Key::Escape, pressed: true, .. }));
                        let click = input.clicked && ok.is_some_and(|r| input.pointer.is_some_and(|p| inside(&r, p)));
                        if key || click {
                            self.login_error = None;
                            if let Some(a) = self.audio.as_mut() {
                                a.ui(crate::audio::Ui::Select);
                            }
                        }
                    }
                    match chosen.filter(|_| self.login_error.is_none()) {
                        Some(LoginChoice::Exit) => self.exit_requested = true,
                        Some(c) => {
                            let create = c == LoginChoice::Create;
                            let (p, o) = (password.clone(), if create { String::new() } else { otp.clone() });
                            action = Some(Box::new(move |app: &mut App| app.login(p, o, create)));
                        }
                        None => {}
                    }
                }
            }
            Phase::Characters { lobby, chars, new_name, race, face, size, job, nation, selected, screen, menu, confirm: _, expansions } => {
                let live: Vec<usize> = chars.iter().enumerate().filter(|(_, c)| !c.is_empty_slot()).map(|(i, _)| i).collect();
                if selected.is_none_or(|i| !live.contains(&i)) {
                    *selected = live.first().copied();
                }
                let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
                if *screen != LobbyScreen::Main {
                    self.title_press = None;
                }
                match screen {
                    LobbyScreen::Main => {
                        const ITEMS: [(&str, &str); 5] = [
                            ("Select Character", "Select a character to log on with."),
                            ("Create Character", "Create a new character."),
                            ("Delete Character", "Delete a character."),
                            ("Config", "Change settings."),
                            ("Back", "Return to the previous screen."),
                        ];
                        const UPDATE: usize = 5;
                        // FUN_10235fa0: while lobycwin (or our settings screen) is up the title takes no input.
                        let config_open = self.lobby_cfg.is_some() || self.lobby_settings.is_some() || self.update_ui.is_some();
                        // Not retail: Update (menu 5) above Select Character while something is out of date.
                        let upd = self.updates.as_ref().is_some_and(|s| s.lock().unwrap().any());
                        if !upd && *menu == UPDATE {
                            *menu = 0;
                        }
                        let order: &[usize] = if upd { &[UPDATE, 0, 1, 2, 3, 4] } else { &[0, 1, 2, 3, 4] };
                        // FUN_102406d0: Config (and our Update window) give the title back when they close.
                        if !config_open && self.title_press.is_some_and(|p| p.2 && (p.0 == 3 || p.0 == UPDATE)) {
                            self.title_press = None;
                        }
                        // FUN_102405f0: a taken press locks the title (+0x1c, window +0x5d) until the screen changes.
                        let config_open = config_open || self.title_press.is_some_and(|p| p.2);
                        if !config_open {
                            ctx.input(|i| {
                                let at = order.iter().position(|&m| m == *menu).unwrap_or(0);
                                if i.key_pressed(egui::Key::ArrowDown) {
                                    *menu = order[(at + 1) % order.len()];
                                }
                                if i.key_pressed(egui::Key::ArrowUp) {
                                    *menu = order[(at + order.len() - 1) % order.len()];
                                }
                            });
                        }
                        let mut chosen = ctx.input(|i| i.key_pressed(egui::Key::Enter)).then_some(*menu).filter(|_| !config_open);
                        // FUN_102333b0: Select / Delete need a character, Create a free slot.
                        let enabled = [!live.is_empty(), chars.iter().any(|c| c.is_empty_slot()), !live.is_empty(), true, true];
                        if let Some(lu) = self.game_ui.as_mut() {
                            self.ui_drawn = true;
                            let (sw, sh) = (screen_size[0] as f32, screen_size[1] as f32);
                            // Our menu is in screen order (Select, Create, Delete, Config, Back); loby2win's children
                            // are Select, Create, Delete, Back, Config (FUN_102357b0), Config drawn above Back.
                            const CHILD: [usize; 5] = [0, 1, 2, 4, 3];
                            // Logos need both the server's bit and the install's (FUN_10235810, simplified).
                            let logos = *expansions & self.installed;
                            let mut rects: Vec<(usize, [f32; 4])> = lu.button_rects(sw, sh, logos).into_iter().enumerate().map(|(i, r)| (CHILD[i], r)).collect();
                            if upd {
                                rects.extend(lu.update_button_rect(sw, sh, logos).map(|r| (UPDATE, r)));
                            }
                            for (m, r) in &rects {
                                if !config_open && input.pointer.is_some_and(|p| inside(r, p)) {
                                    lu.hover.set(true);
                                    if input.moved {
                                        *menu = *m;
                                    }
                                    if input.clicked {
                                        chosen = Some(*m);
                                    }
                                }
                            }
                            let child_enabled = CHILD.map(|k| enabled[k]);
                            let shown = if *menu == UPDATE { 0 } else { CHILD[*menu] };
                            let wardrobes = lobby.as_ref().map_or(0, |l| l.features);
                            let press = self.title_press.filter(|p| p.0 == *menu && p.0 != UPDATE).map(|p| ((9.0 - p.1.elapsed().as_secs_f32() * 60.0).max(0.0), p.2));
                            let slots = lu.title_update(sw, sh, Some(shown), logos, child_enabled, upd.then_some((*menu == UPDATE, true)), wardrobes, press);
                            let help = match (&self.update_ui, &self.lobby_settings, &self.lobby_cfg) {
                                (Some(u), _, _) => u.help(),
                                (None, Some(s), _) => s.help(),
                                (None, None, Some(c)) => match c.focus() {
                                    Some((_, configui::SETTINGS)) => configui::SETTINGS_HELP.to_string(),
                                    // lobyhelp ids 0x24.. (FUN_10236b40) -> group 8 0xf5..0x101 by child.
                                    Some((_, child)) => {
                                        let id = match child {
                                            1..=4 => 0xf4 + child as usize,
                                            5 => 0xfb,
                                            6 => 0x101,
                                            c => 0xf5 + c as usize,
                                        };
                                        lu.lobby_msg(8, id, "")
                                    }
                                    None => String::new(),
                                },
                                _ if *menu == UPDATE => "Update the game files or vana-client.".to_string(),
                                _ => ITEMS[*menu].1.to_string(),
                            };
                            lobby_text(lu, &slots, &help, WHITE, &self.cfg.client_version);
                            // lobycwin and our settings screen over the title.
                            if let Some(c) = self.lobby_cfg.as_mut() {
                                c.owned = self.installed;
                                if self.lobby_settings.is_none() {
                                    let mut outs = Vec::new();
                                    for ev in &input.events {
                                        if let egui::Event::Key { key, pressed: true, .. } = ev {
                                            outs.push(c.key(lu, &mut self.lobby_values, *key, sw));
                                        }
                                    }
                                    if let Some(p) = input.pointer {
                                        outs.push(c.pointer(lu, &mut self.lobby_values, p, input.moved, input.clicked, sw, sh));
                                    }
                                    for out in outs {
                                        if let Some(a) = self.audio.as_mut() {
                                            for s in &out.sounds {
                                                a.ui(*s);
                                            }
                                        }
                                        lobby_changes(&out.changed, &mut lobby_apply);
                                        if out.open_settings {
                                            let mut s = settings::SettingsUi::open(&self.cfg, window);
                                            s.monitors = self.monitors.clone();
                                            s.gpus = self.gpus.clone();
                                            let v = self.lobby_values.get(0xa8);
                                            (s.s.music, s.s.effects) = (v, v);
                                            self.lobby_settings = Some(s);
                                            settings_new = true;
                                        }
                                        if out.closed {
                                            self.lobby_cfg = None;
                                            self.cfg.game.store(None, &self.lobby_values);
                                            save = true;
                                            break;
                                        }
                                    }
                                }
                                if let Some(c) = &self.lobby_cfg {
                                    c.draw(lu, &self.lobby_values, sw, sh);
                                }
                            }
                            if let Some(s) = self.lobby_settings.as_mut() {
                                let mut closed = false;
                                // The Enter that opened it is not its first key.
                                for ev in input.events.iter().filter(|_| !settings_new) {
                                    if let egui::Event::Key { key, pressed: true, modifiers, .. } = ev {
                                        closed |= s.key_mod(*key, modifiers.ctrl || modifiers.alt);
                                        if let Some(a) = self.audio.as_mut() {
                                            for x in s.sounds.drain(..) {
                                                a.ui(x);
                                            }
                                        }
                                    }
                                }
                                if let Some(p) = input.pointer.filter(|_| !settings_new) {
                                    s.pointer(lu, p, input.moved, input.clicked, sw, sh);
                                    if s.hover {
                                        lu.hover.set(true);
                                    }
                                    if let Some(a) = self.audio.as_mut() {
                                        for x in s.sounds.drain(..) {
                                            a.ui(x);
                                        }
                                    }
                                }
                                if s.dirty {
                                    s.dirty = false;
                                    s.s.store(&mut self.cfg);
                                    // The lobby has one volume (0xa8): the row changed sets both.
                                    let old = self.lobby_values.get(0xa8);
                                    let v = if s.s.music != old { s.s.music } else { s.s.effects };
                                    (s.s.music, s.s.effects) = (v, v);
                                    self.lobby_values.set(0xa8, v);
                                    self.cfg.game.store(None, &self.lobby_values);
                                    lobby_changes(&[0xa8], &mut lobby_apply);
                                    apply = true;
                                    save = true;
                                }
                                s.draw(lu, sw, sh);
                                if closed {
                                    self.lobby_settings = None;
                                    if let Some(a) = self.audio.as_mut() {
                                        a.ui(crate::audio::Ui::Cancel);
                                    }
                                }
                            }
                            if let Some(u) = self.update_ui.as_mut() {
                                let mut outs = vec![u.poll()];
                                for ev in input.events.iter() {
                                    if let egui::Event::Key { key, pressed: true, .. } = ev {
                                        outs.push(u.key(*key));
                                    }
                                }
                                if let Some(p) = input.pointer {
                                    outs.push(u.pointer(p, input.moved, input.clicked, sw, sh));
                                    if outs.last().is_some_and(|o| o.hover) {
                                        lu.hover.set(true);
                                    }
                                }
                                u.draw(lu, sw, sh);
                                for out in outs {
                                    if let Some(a) = self.audio.as_mut() {
                                        for x in &out.sounds {
                                            a.ui(*x);
                                        }
                                    }
                                    // New and changed DATs: the next login opens the index again.
                                    if out.reload {
                                        self.index = None;
                                    }
                                    if out.restart {
                                        if let Ok(exe) = std::env::current_exe() {
                                            let _ = std::process::Command::new(exe).args(std::env::args().skip(1)).spawn();
                                        }
                                        self.exit_requested = true;
                                    }
                                    if out.closed {
                                        self.update_ui = None;
                                        break;
                                    }
                                }
                            }
                        }
                        // FUN_10235ec0 case 5: the press counter starts (the generic handler, window +0x5e = 9); a
                        // disabled button (Create excepted) only buzzes, any other is taken with 0002 (the fade hides
                        // the screen change from the UiSnap diff).
                        if let Some(i) = chosen {
                            let refused = i < 5 && i != 1 && !enabled[i];
                            self.title_press = Some((i, Instant::now(), !refused));
                            if let Some(a) = self.audio.as_mut() {
                                a.ui(if refused { crate::audio::Ui::Error } else { crate::audio::Ui::Select });
                            }
                            if refused {
                                chosen = None;
                            }
                        }
                        match chosen {
                            Some(UPDATE) => {
                                if let Some(s) = self.updates.clone() {
                                    self.update_ui = Some(updater::UpdateUi::open(s, self.cfg.ffxi_dir()));
                                }
                            }
                            Some(0) => *screen = LobbyScreen::Select,
                            Some(1) => {
                                *screen = LobbyScreen::Create;
                                self.creator_step = 0;
                            }
                            Some(2) => *screen = LobbyScreen::Delete,
                            // FUN_10235fa0 index 4: lobycwin over the title (FUN_10195e80 reads the values first).
                            Some(3) => {
                                self.lobby_values = self.cfg.game.for_character(None);
                                self.lobby_cfg = Some(configui::ConfigUi::open("lobycwin"));
                            }
                            Some(4) => {
                                action = Some(Box::new(|app: &mut App| app.phase = Phase::login(&app.cfg)));
                                fade_action = true;
                            }
                            _ => {}
                        }
                        // Escape does not leave: the cursor goes to Back (0001).
                        if escape && !config_open {
                            *menu = 4;
                            if let Some(a) = self.audio.as_mut() {
                                a.ui(crate::audio::Ui::Cursor);
                            }
                        }
                    }
                    LobbyScreen::Select | LobbyScreen::Delete if self.game_ui.is_some() => {
                        // FUN_10238ea0 / FUN_10230a10 through client/select.rs.
                        let deleting = *screen == LobbyScreen::Delete;
                        let gu = self.game_ui.as_mut().unwrap();
                        self.ui_drawn = true;
                        let (sw, sh) = (screen_size[0] as f32, screen_size[1] as f32);
                        let list = select::List::new(chars);
                        let shown = if busy { Some(("Connecting to the server...", WHITE)) } else if !status.is_empty() { Some((status.as_str(), YELLOW)) } else { None };
                        let sin = select::Input { events: &input.events, pointer: input.pointer, moved: input.moved, clicked: input.clicked, busy };
                        let out = self.select_ui.step(gu, &sin, &list, chars, deleting, sw, sh, shown);
                        if let Some(a) = self.audio.as_mut() {
                            for s in &out.sounds {
                                a.ui(*s);
                            }
                        }
                        if let Some(sel) = self.select.as_mut() {
                            if out.input {
                                sel.input();
                            }
                            if let Some(c) = out.moved {
                                sel.cursor_moved(c);
                            }
                        }
                        *selected = list.entries.get(self.select_ui.cursor).copied();
                        match out.action {
                            Some(select::Action::Login(i)) => {
                                if let (Some(l), Some(c)) = (lobby.take(), chars.get(i).cloned()) {
                                    action = Some(Box::new(move |app: &mut App| app.spawn(move || Task::Selected(l.select(&c)))));
                                    fade_action = true;
                                }
                            }
                            Some(select::Action::Rename(i, name)) => {
                                // LSB view 0x28, then the login with the new name (LAB_102395b5).
                                if let (Some(mut l), Some(mut c)) = (lobby.take(), chars.get(i).cloned()) {
                                    action = Some(Box::new(move |app: &mut App| {
                                        app.spawn(move || {
                                            Task::Selected((|| {
                                                l.rename(&c, &name)?;
                                                c.name = name;
                                                l.select(&c)
                                            })())
                                        })
                                    }));
                                    fade_action = true;
                                }
                            }
                            Some(select::Action::Delete(i)) => {
                                if let (Some(mut l), Some(c)) = (lobby.take(), chars.get(i).cloned()) {
                                    action = Some(Box::new(move |app: &mut App| {
                                        app.spawn(move || {
                                            Task::Lobby((|| {
                                                l.delete_character(&c)?;
                                                let chars = l.characters()?;
                                                Ok((l, chars))
                                            })())
                                        })
                                    }));
                                }
                            }
                            Some(select::Action::Back) => *screen = LobbyScreen::Main,
                            None => {}
                        }
                    }
                    LobbyScreen::Create if self.game_ui.is_some() && self.creator.is_some() => {
                        // Steps and their menus (0x30 layouts); help lines from groups 8 / 0x1b (creator::draw_ui).
                        use creator::STEPS;
                        let gu = self.game_ui.as_mut().unwrap();
                        self.ui_drawn = true;
                        let (sw, sh) = (screen_size[0] as f32, screen_size[1] as f32);
                        let step = self.creator_step.min(7);
                        let n = STEPS.get(step).map_or(0, |s| s.1);
                        let mut sel = match step {
                            0 => *race,
                            1 => *face as usize / 2,
                            2 => *face as usize % 2,
                            3 => *size as usize,
                            4 => *job,
                            6 => *nation,
                            _ => 0,
                        };
                        let (mut next, back, mut answer) = (false, escape, None);
                        // FUN_102384d0: Num+ (DIK 0x4e) on the name step asks for a random name.
                        if step == 5
                            && self.input.pressed(0x4e)
                            && let Some(index) = self.index.as_ref()
                        {
                            let mut b = [0u8; 4];
                            let mut rand = || {
                                let _ = getrandom::fill(&mut b);
                                u32::from_le_bytes(b) & 0x7fff
                            };
                            if let Some(n) = creator::random_name(index, &mut rand) {
                                *new_name = n;
                            }
                        }
                        for ev in &input.events {
                            match ev {
                                egui::Event::Text(t) | egui::Event::Paste(t) if step == 5 => {
                                    new_name.extend(t.chars().filter(|c| c.is_ascii_alphabetic()));
                                    new_name.truncate(15);
                                    // FUN_1022dc60: first letter upper case, the rest lower.
                                    *new_name = new_name.chars().enumerate().map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() }).collect();
                                }
                                egui::Event::Key { key, pressed: true, .. } => match key {
                                    egui::Key::Backspace if step == 5 => {
                                        new_name.pop();
                                    }
                                    egui::Key::ArrowDown if n > 0 => sel = (sel + 1) % n,
                                    egui::Key::ArrowUp if n > 0 => sel = (sel + n - 1) % n,
                                    egui::Key::ArrowLeft | egui::Key::ArrowRight if step == 7 => self.creator_yes = !self.creator_yes,
                                    egui::Key::Enter if step == 7 => answer = Some(self.creator_yes),
                                    egui::Key::Enter => next = true,
                                    // View modes cycle 0 -> 1 -> 2 (FUN_10243450).
                                    egui::Key::Tab => {
                                        if let Some(cr) = self.creator.as_mut() {
                                            cr.next_view();
                                        }
                                    }
                                    _ => {}
                                },
                                _ => {}
                            }
                        }
                        gu.clear();
                        if step >= 6 && self.nation.fade < 1.0 {
                            gu.rect(0.0, 0.0, sw, sh, [0.0, 0.0, 0.0, 1.0 - self.nation.fade]);
                        }
                        let view = game_ui::CreatorView { step, race: *race, face: *face as usize / 2, hair: *face as usize % 2, size: *size as usize, job: *job, nation: *nation, name: new_name };
                        let rects = creator::draw_ui(gu, &view, sw, sh, (!status.is_empty()).then_some(status.as_str()), None);
                        if step < 7 {
                            if let Some(p) = input.pointer
                                && let Some(i) = rects.iter().position(|r| inside(r, p))
                            {
                                gu.hover.set(true);
                                if input.moved || input.clicked {
                                    sel = i;
                                }
                                if input.clicked {
                                    next = true;
                                }
                            }
                        } else if let Some(a) = yes_no(gu, &input, sw, sh, &format!("Create {new_name} as {} {}?", RACES[*race], JOBS[*job]), &mut self.creator_yes) {
                            answer = Some(a);
                        }
                        match step {
                            0 => *race = sel,
                            1 => *face = (sel * 2 + *face as usize % 2) as u8,
                            2 => *face = (*face as usize / 2 * 2 + sel) as u8,
                            3 => *size = sel as u8,
                            4 => *job = sel,
                            6 => *nation = sel,
                            _ => {}
                        }
                        if next {
                            if step == 5 && !(3..=15).contains(&new_name.len()) {
                                action = Some(Box::new(|app: &mut App| app.status = "Names are 3 to 15 letters.".into()));
                            } else {
                                self.creator_step = step + 1;
                                self.creator_yes = true;
                                action = Some(Box::new(|app: &mut App| app.status.clear()));
                            }
                        }
                        if back {
                            if step == 0 {
                                // FUN_10237730 plays 0003 itself; the fade hides the change from the UiSnap diff.
                                *screen = LobbyScreen::Main;
                                if let Some(a) = self.audio.as_mut() {
                                    a.ui(crate::audio::Ui::Cancel);
                                }
                            } else {
                                self.creator_step = step - 1;
                            }
                        }
                        let free = chars.iter().filter(|c| c.is_empty_slot()).count();
                        match answer {
                            Some(true) if !busy && free > 0 => {
                                self.music_created = true;
                                if let Some(mut l) = lobby.take() {
                                    // LSB face byte = face * 2 + hair.
                                    let (nm, r, f, sz, j, na) = (new_name.clone(), *race as u8 + 1, *face, *size, *job as u8 + 1, *nation as u8);
                                    // Retail goes straight in with the new character (LSB login_helpers: "We never see the
                                    // character", so its list entry carries only the name and ids) *(inf: from LSB)*.
                                    fade_action = true;
                                    action = Some(Box::new(move |app: &mut App| {
                                        app.spawn(move || {
                                            let made = (|| -> Result<_> {
                                                l.create_character(&NewCharacter { name: &nm, race: r, face: f, size: sz, job: j, nation: na })?;
                                                let chars = l.characters()?;
                                                Ok((l, chars))
                                            })();
                                            match made {
                                                Ok((l, chars)) => match chars.iter().find(|c| c.name.eq_ignore_ascii_case(&nm)).cloned() {
                                                    Some(c) => Task::Selected(l.select(&c)),
                                                    None => Task::Lobby(Ok((l, chars))),
                                                },
                                                Err(e) => Task::Lobby(Err(e)),
                                            }
                                        })
                                    }));
                                }
                            }
                            Some(_) => self.creator_step = 6,
                            None => {}
                        }
                    }
                    // The creator and its scene load behind the lobby fade (FUN_10245fb0 fades the scene in once
                    // the dancer shows); Config has no game screen yet.
                    _ => {}
                }
            }
            Phase::World(world) => {
                let aspect = cfgapply::aspect(window, self.cfg.retail_aspect, world.game.get(0x43));
                let view_proj = Self::camera(world).view_proj(aspect);
                // Debug lines (F12); retail has none. Lines never wrap: an Area alone sizes to almost nothing.
                if self.debug_overlay {
                    let mut lines = vec![format!("{}  zone {}  ({:.1}, {:.1}, {:.1})  status {}", world.name, world.zone.map_or("-".into(), |z| z.to_string()), world.pos.x, world.pos.y, world.pos.z, world.my_motion.status)];
                    lines.extend(self.perf.lines());
                    if let Some(t) = world.target.and_then(|t| world.entities.get(&t).map(|e| (t, e))) {
                        lines.push(format!("target {:#x} {}  {:.1} away{}", t.0, t.1.name, (t.1.pos - world.pos).length(), if world.locked { "  locked" } else { "" }));
                    }
                    if !status.is_empty() {
                        lines.push(status.clone());
                    }
                    // What holds the keys now (intermittent stuck-input reports).
                    let w = &*world;
                    let open: Vec<&str> = [
                        ("menu", w.menu.is_some()),
                        ("confirm", w.confirm.is_some()),
                        ("items", w.win.is_some()),
                        ("action", w.action_menu.is_some()),
                        ("config", w.config.is_some()),
                        ("settings", w.settings.is_some()),
                        ("map", w.map.is_some()),
                        ("fulllog", w.full_log.is_some()),
                        ("magic", w.magic.is_some()),
                        ("ability", w.ability.is_some()),
                        ("cats", w.cats.is_some()),
                        ("moogle", w.moogle.is_some()),
                        ("mogdoor", w.mogdoor.is_some()),
                        ("shop", shop::active(w)),
                        ("trade", trade::active(w)),
                        ("post", post::active(w)),
                        ("loot", treasure::active(w)),
                        ("synth", synth::active(w)),
                        ("search", w.search.is_some()),
                        ("inspect", w.inspect.up()),
                        ("prompt", w.prompt.is_some()),
                        ("typing", w.typing),
                        ("log_active", w.log_active),
                        ("ko_open", w.ko_open),
                        ("raise", w.ko_offer == 2),
                        ("event", event::active(w)),
                        ("choice", event::choice(w).is_some()),
                        ("palette", w.palette.0 != 0),
                    ]
                    .into_iter()
                    .filter(|(_, on)| *on)
                    .map(|(n, _)| n)
                    .collect();
                    lines.push(format!("open [{}]  log_focus {} (active {})  locked {}  party_cursor {}", open.join(" "), w.log_focus, w.log_active, w.locked, w.party_cursor.is_some()));
                    lines.push(self.input.debug_state());
                    egui::Area::new(egui::Id::unique("status")).anchor(egui::Align2::LEFT_TOP, [8.0, 56.0]).interactable(false).show(ctx, |ui| {
                        egui::Frame::new().fill(egui::Color32::from_black_alpha(150)).inner_margin(4.0).show(ui, |ui| {
                            for l in &lines {
                                ui.add(egui::Label::new(egui::RichText::new(l).monospace().color(egui::Color32::WHITE)).extend());
                            }
                        });
                    });
                }
                if let Some(gu) = self.game_ui.as_mut() {
                    self.ui_drawn = true;
                    gu.set_style(world.game.get(0x12) as usize);
                    self.hud_blockers = world_hud(gu, world, &input, screen_size, &mut self.keys, view_proj, self.numpad_slash);
                }
                // What the Config windows and the settings screen hand to the app.
                if let Some(kind) = world.want_settings.take() {
                    // Size lists come from the window, not the menu layout it is scaled to.
                    let mut s = settings::SettingsUi::open_kind(&self.cfg, window, kind);
                    s.monitors = self.monitors.clone();
                    s.gpus = self.gpus.clone();
                    s.in_game = true;
                    world.settings = Some(s);
                }
                if let Some(s) = world.settings.as_mut().filter(|s| s.dirty) {
                    s.dirty = false;
                    s.s.store(&mut self.cfg);
                    apply = true;
                }
                if std::mem::take(&mut world.volume_dirty) {
                    apply = true;
                }
                if std::mem::take(&mut world.party_tp_save) {
                    self.cfg.party_tp = world.party_tp;
                    if let Err(e) = self.cfg.save() {
                        log::warn!("config: {e:#}");
                    }
                }
                if std::mem::take(&mut world.game_save) || apply {
                    self.cfg.game.store(Some(&world.name), &world.game);
                    save = true;
                }
            }
        }
        if lobby_apply.music
            && let (Some(a), Phase::Characters { .. }) = (self.audio.as_mut(), &self.phase)
        {
            // FUN_1023ae50: a new title music choice starts at once.
            a.title_music(self.lobby_values.get(0xaf), self.installed);
        }
        if lobby_apply.volume {
            self.cfg.game.store(None, &self.lobby_values);
            apply = true;
        }
        if apply {
            self.apply_settings();
        }
        if save && let Err(e) = self.cfg.save() {
            log::error!("saving the config: {e:#}");
        }
        // Lobby screen changes and leaving the lobby happen once faded out (FUN_10233d50 .. FUN_10233cf0).
        if let (Some(old), Phase::Characters { screen, .. }) = (before, &mut self.phase)
            && *screen != old
        {
            let new = std::mem::replace(screen, old);
            if !fading {
                self.lobby_fade.out(Box::new(move |app: &mut App| {
                    if let Phase::Characters { screen, .. } = &mut app.phase {
                        *screen = new;
                    }
                    // FUN_10238580 rebuilds the scene and FUN_102371c0 starts the cursor at 0 on every opening.
                    if matches!(new, LobbyScreen::Select | LobbyScreen::Delete) {
                        app.select_ui = select::SelectUi::default();
                        if let Some(s) = app.select.as_mut() {
                            s.reset();
                        }
                    }
                    app.lobby_fade.enter(new != LobbyScreen::Create);
                }));
            }
        }
        if let Some(a) = action {
            if !fade_action {
                a(self);
            } else if !fading {
                self.lobby_fade.out(Box::new(move |app: &mut App| {
                    a(app);
                    app.lobby_fade.enter(true);
                }));
            }
        }
    }

    /// Advances a scripted run; returns true once the screenshot should be taken this frame.
    fn step_drive(&mut self) -> bool {
        if self.busy {
            return false;
        }
        let drive_done = self.drive_done();
        match &mut self.phase {
            Phase::Login { .. } if !self.drive.started => {
                if let (Some((user, pass)), false) = (self.drive.login.clone(), self.drive.input.as_ref().is_some_and(|s| s.logs_in)) {
                    self.drive.started = true;
                    self.cfg.username = user;
                    self.login(pass, String::new(), false);
                } else if self.drive.screenshot.is_some() && !self.drive.shot_taken && self.game_ui.is_some() {
                    // No credentials: photograph the login screen.
                    let ready = *self.drive.zone_ready_at.get_or_insert_with(Instant::now);
                    if ready.elapsed().as_secs_f32() >= self.drive.screenshot_after {
                        self.drive.shot_taken = true;
                        return true;
                    }
                }
            }
            Phase::Characters { .. } if self.drive.character.is_none() => {
                // No character to play: photograph the select screen once its model is up.
                if self.drive.screenshot.is_some() && !self.drive.shot_taken {
                    let ready = *self.drive.zone_ready_at.get_or_insert_with(Instant::now);
                    if ready.elapsed().as_secs_f32() >= self.drive.screenshot_after {
                        self.drive.shot_taken = true;
                        return true;
                    }
                }
            }
            // A script that logs in presses Select Character itself; the hold starts on the select screen.
            Phase::Characters { screen, chars, .. } if self.drive.relogin && self.drive.pick_after.is_none_or(|t| Instant::now() < t) => {
                if *screen == LobbyScreen::Main && !chars.is_empty() && !self.drive.input.as_ref().is_some_and(|s| s.logs_in) {
                    *screen = LobbyScreen::Select;
                }
                if *screen == LobbyScreen::Select && self.drive.pick_after.is_none() {
                    self.drive.pick_after = Some(Instant::now() + std::time::Duration::from_secs(8));
                }
            }
            // A script that logs in walks the title menu itself; pick the character once it reaches Select.
            Phase::Characters { lobby, chars, screen, .. } if !self.drive.input.as_ref().is_some_and(|s| s.logs_in) || *screen == LobbyScreen::Select => {
                if let (Some(name), Some(_)) = (self.drive.character.clone(), lobby.as_ref()) {
                    if let Some(c) = chars.iter().find(|c| c.name.eq_ignore_ascii_case(&name)).cloned() {
                        self.drive.relogin = false;
                        let l = lobby.take().unwrap();
                        self.spawn(move || Task::Selected(l.select(&c)));
                    }
                }
            }
            Phase::World(world) if world.renderer.is_some() && world.zone_uploading.is_none() => {
                // Test runs end with "/shutdown on" (0x0E7 mode 3, kind 3); the server refuses it in an event, so once that is over.
                if self.drive.quit && drive_done && !world.logging_out && !event::active(world) {
                    world.logging_out = true;
                    world.shutdown = true;
                    let _ = world.net.commands.send(NetCommand::Logout { mode: 3, kind: 3 });
                }
                let ready = *self.drive.zone_ready_at.get_or_insert_with(Instant::now);
                let t = ready.elapsed().as_secs_f32();
                if *self.drive.goto_zone.get_or_insert(world.pos_zone) != world.pos_zone && !self.drive.goto.is_empty() {
                    self.drive.goto.clear();
                    self.keys.remove(&KeyCode::KeyW);
                }
                // While engaged, or with a monster targeted (to get in /attack range).
                let mob_target = world.target.and_then(|t| world.entities.get(&t)).is_some_and(|e| e.mob);
                if self.drive.chase && self.drive.goto.is_empty() && (world.my_motion.status == 1 || mob_target) {
                    match world.target.and_then(|t| world.entities.get(&t)).map(|e| e.pos - world.pos) {
                        Some(to) if to.length() > 2.0 => {
                            camera::face(world, to.x.atan2(to.z));
                            self.keys.insert(KeyCode::KeyW);
                        }
                        _ => {
                            self.keys.remove(&KeyCode::KeyW);
                        }
                    }
                }
                if let Some(&(x, z)) = self.drive.goto.first() {
                    let to = Vec3::new(x, world.pos.y, z) - world.pos;
                    if to.length() < 0.6 {
                        self.drive.goto.remove(0);
                        self.keys.remove(&KeyCode::KeyW);
                    } else {
                        camera::face(world, to.x.atan2(to.z));
                        self.keys.insert(KeyCode::KeyW);
                    }
                }
                if t < self.drive.walk_secs {
                    self.keys.insert(KeyCode::KeyW);
                } else if self.drive.walk_secs > 0.0 {
                    self.keys.remove(&KeyCode::KeyW);
                }
                if self.drive.screenshot.is_some() && !self.drive.shot_taken && t >= self.drive.screenshot_after {
                    self.drive.shot_taken = true;
                    let cam = Self::camera(world);
                    let head = world.pos + Vec3::new(0.0, -1.5, 0.0);
                    let g = world.ground.as_ref();
                    log::info!(
                        "drive: pos {} yaw {:.2} eye {} hit {:?} floor-here {:?} floor-below-anything {:?}",
                        world.pos, world.yaw, cam.eye,
                        g.and_then(|g| g.raycast(head, cam.eye)),
                        g.and_then(|g| g.floor(world.pos.x, world.pos.z, world.pos.y, 1.2)),
                        g.and_then(|g| g.floor(world.pos.x, world.pos.z, world.pos.y, 50.0)),
                    );
                    let mut near: Vec<(f32, &str, Vec3)> = world.entities.values().filter(|e| !e.name.is_empty()).map(|e| ((e.pos - world.pos).length(), e.name.as_str(), e.pos)).collect();
                    near.sort_by(|a, b| a.0.total_cmp(&b.0));
                    log::info!("drive: nearest {:?}", &near[..near.len().min(6)]);
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    /// Where the client is, for `until:` steps of a drive script.
    fn phase_name(&self) -> &'static str {
        match &self.phase {
            Phase::Login { .. } => "login",
            Phase::Characters { screen: LobbyScreen::Main, .. } => "title",
            Phase::Characters { .. } => "lobby",
            Phase::World(w) if w.event.is_some() => "event",
            Phase::World(w) if w.renderer.is_some() && w.zone_uploading.is_none() && w.fade == 0.0 && w.in_scene() && w.event_pending.is_none() => "world",
            Phase::World(_) => "loading",
        }
    }

    /// Parses the lobby DATs and font on a worker thread.
    fn load_game_ui(&mut self) {
        self.game_ui_state = LoadState::Loading;
        if firstrun::needed(&self.cfg.ffxi_dir()) {
            self.first_run = Some(firstrun::FirstRun::new(self.cfg.game_dir.clone()));
            self.game_ui_state = LoadState::Failed;
            return;
        }
        let index = match self.game_data() {
            Ok((index, _)) => index,
            Err(e) => {
                log::warn!("lobby UI: {e:#}");
                self.game_ui_state = LoadState::Failed;
                return;
            }
        };
        let dll = self.cfg.ffxi_dir().join("FFXiMain.dll");
        let tx = self.tasks_tx.clone();
        // Config-tool 0036 (FUN_10119400): the font texture.
        let quality = match self.cfg.tool("0036") {
            0 => crate::dat::font::Quality::Compressed,
            1 => crate::dat::font::Quality::Uncompressed,
            _ => crate::dat::font::Quality::High,
        };
        std::thread::spawn(move || {
            let data = std::fs::read(&dll).map_err(anyhow::Error::from).and_then(|b| GameUiData::load_with(&index, &b, quality));
            let _ = tx.send(Task::GameUi(data));
        });
    }

    /// Loads a random title movie and its zone on a worker thread.
    fn load_flyover(&mut self) {
        self.flyover_state = LoadState::Loading;
        let Ok((index, keys)) = self.game_data() else {
            self.flyover_state = LoadState::Failed;
            return;
        };
        let tx = self.tasks_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Task::Flyover(flyover::FlyoverData::load(&index, &keys, flyover::pick_movie())));
        });
    }

    /// The login screen and title menu play the flyover behind the lobby art.
    fn title_backdrop(&self) -> bool {
        matches!(self.phase, Phase::Login { .. } | Phase::Characters { screen: LobbyScreen::Main, .. })
    }

    fn redraw(&mut self) {
        let t0 = Instant::now();
        if self.game_ui_state == LoadState::Idle {
            self.load_game_ui();
        }
        if self.title_backdrop() {
            if self.flyover.is_none() && self.flyover_state == LoadState::Idle {
                self.load_flyover();
            }
        } else if matches!(self.phase, Phase::World(_)) {
            self.flyover = None;
        }
        let in_select = matches!(&self.phase, Phase::Characters { screen: LobbyScreen::Select | LobbyScreen::Delete, .. });
        if let (true, Phase::Characters { chars, .. }) = (in_select, &self.phase) {
            let key: String = chars.iter().map(|c| format!("{}:{:?};", c.name, c.looks)).collect();
            if key != self.select_key && self.select_state != LoadState::Loading && self.select_state != LoadState::Failed {
                // Ring slot i holds list entry i (named characters only, FUN_102333b0).
                let list = select::List::new(chars);
                let looks: Vec<Option<(Look, u8)>> = (0..select::SLOTS)
                    .map(|i| {
                        list.get(chars, i).map(|c| {
                            let l = &c.looks;
                            (Look { race: c.race as u8, face: c.face, head: l[1], body: l[2], hands: l[3], legs: l[4], feet: l[5], main: l[6], sub: l[7], range: 0 }, c.size)
                        })
                    })
                    .collect();
                if let Ok((index, keys)) = self.game_data() {
                    self.select_state = LoadState::Loading;
                    let tx = self.tasks_tx.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(Task::Select(select::SelectData::load(&index, &keys, &looks), key));
                    });
                }
            }
        } else if !matches!(self.phase, Phase::Characters { .. }) {
            self.select = None;
            self.select_key.clear();
        }
        let in_create = matches!(&self.phase, Phase::Characters { screen: LobbyScreen::Create, .. });
        if in_create {
            if self.creator.is_none() && self.creator_state == LoadState::Idle
                && let Ok((index, keys)) = self.game_data()
            {
                self.creator_state = LoadState::Loading;
                let tx = self.tasks_tx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(Task::CreatorBackdrop(creator::Backdrop::load(&index, &keys)));
                });
            }
            if let (Some(cr), Phase::Characters { race, face, .. }) = (&mut self.creator, &self.phase) {
                let want = (*race, *face as usize / 2);
                cr.hair = *face as usize % 2;
                if cr.wants() != Some(want) && self.dancer_loading != Some(want)
                    && let Some(index) = self.index.clone()
                {
                    self.dancer_loading = Some(want);
                    // FUN_10036b40: the old music fades while the race loads; it starts with the performance.
                    if let Some(a) = &mut self.audio {
                        a.prepare_music(CREATOR_BGM[want.0.min(7)]);
                    }
                    let tx = self.tasks_tx.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(Task::Dancer(creator::Dancer::load(&index, want.0, want.1)));
                    });
                }
            }
        } else if !matches!(self.phase, Phase::Characters { .. }) {
            self.creator = None;
        }
        // Nation step: the highlighted city's flyover (FUN_1022ea40): a 10-frame settle, a 60-tick fade to
        // black ('ffot', file 33193), file 24 + nation loads, a 60-tick fade back ('ffin').
        let nation_step = in_create && self.creator_step >= 6; // the flyover stays up behind the confirm (FUN_10233b50)
        if let (true, Phase::Characters { nation, .. }) = (nation_step, &self.phase) {
            let want = *nation;
            let ns = &mut self.nation;
            let fdt = self.last_frame.elapsed().as_secs_f32().min(0.1);
            if ns.fly.as_ref().is_some_and(|f| f.0 == want) {
                ns.hold = 0.0;
                ns.fade = (ns.fade + fdt).min(1.0);
            } else {
                ns.hold += fdt;
                if ns.hold >= 10.0 / 30.0 {
                    ns.fade = (ns.fade - fdt).max(0.0);
                }
                if ns.fade <= 0.0
                    && ns.loading.is_none()
                    && let Ok((index, keys)) = self.game_data()
                {
                    let ns = &mut self.nation;
                    ns.loading = Some(want);
                    ns.fly = None;
                    let tx = self.tasks_tx.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(Task::Nation(want, flyover::FlyoverData::load_file(&index, &keys, 24 + want as u32, "mov1")));
                    });
                }
            }
        } else if self.nation.fly.is_some() || self.nation.fade < 1.0 {
            self.nation = NationShow::default();
        }
        // A finished movie makes way for another random one.
        if self.flyover.as_ref().is_some_and(|f| f.done) {
            self.flyover = None;
        }
        while let Ok(t) = self.tasks.try_recv() {
            let name = t.name();
            perf::timed(|| format!("task {name}"), || self.handle_task(t));
        }
        perf::timed(|| "network".into(), || self.poll_network());
        self.switch_area();
        self.update_sub_area();
        if let Phase::World(w) = &mut self.phase {
            place_mogdoor(w);
            res_rdy(w);
            moogle::update(w);
            post::update(w);
            treasure::update(w);
            shop::update(w);
            trade::update(w);
            combat::battle_frame(w);
            synth::update(w);
            servmes::frame(w);
        }
        // Nothing is shown until the title can draw: its UI data and flyover (or a failure to say so).
        if !self.shown {
            let ready = self.game_ui_state == LoadState::Failed || (self.game_ui.is_some() && (self.flyover.is_some() || self.flyover_state == LoadState::Failed || !self.title_backdrop()));
            if !ready {
                self.next_frame = Some(Instant::now() + std::time::Duration::from_millis(16));
                return;
            }
            self.shown = true;
            if let Some(g) = &self.gfx {
                g.window.set_visible(true);
            }
            // A full-screen state asked for while the window was hidden is not kept by KWin; ask again once mapped.
            if self.first_run.is_none() {
                self.refullscreen.set(Some(3));
            }
        }
        let shoot = self.step_drive();
        let dt = self.last_frame.elapsed().as_secs_f32().min(0.1);
        self.last_frame = Instant::now();
        self.key_functions(dt);
        perf::timed(|| "movement".into(), || self.update_movement(dt));
        if matches!(&self.phase, Phase::Characters { screen: LobbyScreen::Create, .. }) && self.creator.as_ref().is_some_and(|c| c.wants().is_some()) && self.lobby_fade.next.is_none() {
            self.lobby_fade.scene_to = 1.0;
        }
        // FUN_10230a10 loads the select scene before it fades in (FUN_10233cf0): hold the fade while ours loads.
        if matches!(&self.phase, Phase::Characters { screen: LobbyScreen::Select | LobbyScreen::Delete, .. }) && self.lobby_fade.next.is_none() {
            let up = if self.select.is_some() || self.select_state == LoadState::Failed { 1.0 } else { 0.0 };
            (self.lobby_fade.ui_to, self.lobby_fade.scene_to) = (up, up);
        }
        if let Some(next) = self.lobby_fade.step(dt * 60.0) {
            next(self);
        }
        let m_update = Instant::now();

        let Some(mut gfx) = self.gfx.take() else { return };
        if self.monitors.is_empty() {
            self.monitors = monitor_list(gfx.window.available_monitors());
            log::info!("monitors: {:?}", self.monitors.iter().map(|m| &m.1).collect::<Vec<_>>());
        }
        if std::mem::take(&mut self.window_dirty) {
            self.apply_window(&gfx.window);
        }
        self.move_window(&gfx.window);
        self.request_size(&mut gfx);
        if let Some(n) = self.refullscreen.get() {
            if n > 0 {
                self.refullscreen.set(Some(n - 1));
            } else {
                self.refullscreen.set(None);
                let (size, mode) = (self.cfg.window_size(), self.cfg.window_mode());
                let want = fullscreen(mode, size, pick_monitor(&self.cfg.monitor, gfx.window.current_monitor(), gfx.window.available_monitors(), size));
                if want.is_some() && !(self.drive.driven() && std::env::var_os("FFXI_TEST_WINDOW").is_none()) {
                    log::info!("window: full screen again once shown ({want:?})");
                    gfx.window.set_fullscreen(want);
                }
            }
        }
        if std::mem::take(&mut self.present_dirty) {
            gfx.config.present_mode = present_mode(&gfx.surface.get_capabilities(&gfx.gpu.adapter), self.frame_limit, &gfx.window);
            gfx.surface.configure(&gfx.gpu.device, &gfx.config);
        }
        self.perf.limit = format!("limit {}  present {:?}", if self.frame_limit == 0 { "unlimited".to_string() } else { format!("{} fps", self.frame_limit) }, gfx.config.present_mode);
        let size = [gfx.config.width, gfx.config.height];
        let mut raw = gfx.egui_state.take_egui_input(&gfx.window);
        let phase = self.phase_name();
        let mut script_shot = None;
        if let Some(sc) = &mut self.drive.input {
            let probes: Vec<String> = match &self.phase {
                Phase::World(w) => [event::shot_name(w).map(|n| format!("cam:{n}")), event::message_id(w).map(|i| format!("msg:{i}")), (!event::active(w)).then(|| "noevent".to_string())].into_iter().flatten().collect(),
                _ => Vec::new(),
            };
            let (events, shot, action) = sc.next(phase, &probes, gfx.window.scale_factor() as f32);
            for e in &events {
                self.input.drive_event(e);
            }
            // Script pointer steps go through the same world mouse path as the window's (physical pixels).
            let ppp = gfx.window.scale_factor();
            for e in &events {
                match e {
                    egui::Event::PointerMoved(p) => self.last_cursor = Some((p.x as f64 * ppp, p.y as f64 * ppp)),
                    egui::Event::PointerButton { button, pressed, .. } => {
                        let b = if *button == egui::PointerButton::Secondary { MouseButton::Right } else { MouseButton::Left };
                        self.mouse_button(b, *pressed, false);
                    }
                    _ => {}
                }
            }
            raw.events.extend(events);
            script_shot = shot;
            // Keys egui has no name for.
            // act:dik:XX taps a key by DIK code (hex); act:down:XX / act:up:XX hold and release it.
            let dik = |p: &str| action.as_deref().and_then(|a| a.strip_prefix(p)).and_then(|h| u8::from_str_radix(h, 16).ok());
            match action.as_deref() {
                Some("numpad*") => self.input.tap(0x37),
                Some("numpad+") => self.input.tap(0x4e),
                _ => {}
            }
            if let Some(k) = dik("dik:") {
                self.input.tap(k);
            }
            if let Some(k) = dik("down:") {
                self.input.set(k, true);
            }
            if let Some(k) = dik("up:") {
                self.input.set(k, false);
            }
            // act:relogin (test drives): after a logout the drive logs in again, opens Select Character and
            // waits 8 s there.
            if action.as_deref() == Some("relogin") {
                (self.drive.started, self.drive.zone_ready_at) = (false, None);
                (self.drive.relogin, self.drive.pick_after) = (true, None);
            }
            // Test drives only: target the nearest living monster with a model (Tab picks NPCs first at gates).
            if let (Some("target-mob"), Phase::World(world)) = (action.as_deref(), &mut self.phase) {
                let pos = world.pos;
                let mob = world.entities.iter().filter(|(id, e)| e.mob && !matches!(e.motion.status, 2 | 3) && world.actors.get(id).is_some_and(|a| a.gpu.is_some())).min_by(|a, b| (a.1.pos - pos).length().total_cmp(&(b.1.pos - pos).length())).map(|(id, _)| *id);
                target::set(world, mob);
            }
            // act:wheel:N turns the wheel N notches (up positive): the focused list's, else the camera's.
            if let Some(n) = action.as_deref().and_then(|a| a.strip_prefix("wheel:")).and_then(|n| n.parse::<f32>().ok()) {
                if self.game_ui.as_ref().is_some_and(|g| g.wants_wheel()) {
                    self.list_wheel += n;
                } else {
                    self.wheel += n;
                }
            }
            // act:goto:X,Z adds a waypoint, as --goto does.
            if let Some((x, z)) = action.as_deref().and_then(|a| a.strip_prefix("goto:")).and_then(|a| a.split_once(','))
                && let (Ok(x), Ok(z)) = (x.trim().parse(), z.trim().parse())
            {
                self.drive.goto.push((x, z));
            }
            // act:monitor:NAME picks the monitor as the settings screen does.
            if let Some(m) = action.as_deref().and_then(|a| a.strip_prefix("monitor:")) {
                self.cfg.monitor = m.to_string();
                self.apply_settings();
            }
            // act:tool:KEY=VAL changes a Config-tool value and applies it, as the settings screen does.
            if let Some((k, v)) = action.as_deref().and_then(|a| a.strip_prefix("tool:")).and_then(|a| a.split_once('=')) {
                if let Ok(v) = v.parse() {
                    self.cfg.set_tool(k, v);
                    self.apply_settings();
                }
            }
        }
        // A Print Screen shot from the game (key function 0x55).
        let mut game_shot = false;
        if script_shot.is_none()
            && let Phase::World(w) = &mut self.phase
            && let Some(p) = w.shot_request.take()
        {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            script_shot = Some(p);
            game_shot = true;
        }
        let shoot = shoot || script_shot.is_some();
        let ctx = gfx.egui_ctx.clone();
        let before = self.ui_snap();
        let output = perf::timed(|| "egui".into(), || ctx.run_ui(raw, |ui| self.ui(ui.ctx(), size)));
        let after = self.ui_snap();
        if let (Some(s), Some(a)) = (Self::ui_sound(&before, &after), self.audio.as_mut()) {
            a.ui(s);
        }
        perf::timed(|| "audio".into(), || self.update_audio(dt));
        // Once the game UI draws, the cursor is the game's: its sprite (the system pointer hidden) or, with config
        // 0021, the Windows cursors set after the frame.
        let mut platform = output.platform_output;
        if let Phase::World(w) = &mut self.phase
            && let Some(t) = w.copied.take()
        {
            platform.commands.push(egui::OutputCommand::CopyText(t));
        }
        if self.ui_drawn {
            platform.cursor_icon = if self.cfg.tool("0021") == 0 { egui::CursorIcon::None } else { egui::CursorIcon::Default };
        } else {
            self.mouse.invalidate();
        }
        gfx.egui_state.handle_platform_output(&gfx.window, platform);
        let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
        let m_ui = Instant::now();

        let frame = match gfx.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                gfx.surface.configure(&gfx.gpu.device, &gfx.config);
                self.gfx = Some(gfx);
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = gfx.gpu.device.create_command_encoder(&Default::default());
        // Background resolution: the scene draws offscreen at its own size and is scaled to the window once;
        // the projection keeps the window's aspect.
        let bg = self.cfg.background(size);
        let ratio = match &self.phase {
            Phase::World(w) => w.game.get(0x43),
            _ => self.cfg.game.global.get(0xab),
        };
        let aspect = cfgapply::aspect(size, self.cfg.retail_aspect, ratio);
        // Gamma (0028 base + Gameplay R / G / B, FUN_10016400) relative to the defaults, on the scene in the
        // scaling pass *(not retail: retail ramps the whole screen, full screen only)*.
        let gamma = {
            let base = self.cfg.gamma_base();
            let c = |id: u16| match &self.phase {
                Phase::World(w) => w.game.get(id) as f32,
                _ => 50.0,
            };
            [0x37, 0x38, 0x39].map(|id| ((base + 1.0 + c(id) / 100.0) / 1.5).max(0.01))
        };
        // The world always draws offscreen: through the plain view, in gamma space.
        let world_phase = matches!(self.phase, Phase::World(_));
        let scaled = bg != size || gamma.iter().any(|g| (g - 1.0).abs() > 1e-3) || world_phase;
        let (scene_view, scene_depth, scene_tex) = if scaled {
            self.scene_scale.get_or_insert_with(|| crate::render::scale::SceneScale::new(&gfx.gpu, gfx.config.format)).target(&gfx.gpu, bg)
        } else {
            (view.clone(), gfx.depth.clone(), frame.texture.clone())
        };
        let scene_raw = if world_phase { self.scene_scale.as_ref().and_then(|s| s.raw_view()) } else { None }.unwrap_or_else(|| scene_view.clone());
        let scene_fmt = gfx.config.format.remove_srgb_suffix();
        let m_acquire = Instant::now();
        let mut m_actors = m_acquire;
        let mut counts = perf::Counts::default();

        let mut drew_world = false;
        if let Phase::World(world) = &mut self.phase {
            let w: &mut World = world;
            let camera = Self::camera(w);
            let mut hidden = event::hidden(w);
            // Misc.: Character Models Displayed (0x3c, 25..50; FUN_10081680): the nearest other PCs only *(inf: which
            // ones retail drops)*.
            let cap = w.game.get(0x3c) as usize;
            let mut pcs: Vec<(f32, u32)> = w.entities.iter().filter(|(id, e)| **id != w.my_id && e.kind == EntityKind::Player).map(|(id, e)| ((e.pos - w.pos).length(), *id)).collect();
            if pcs.len() > cap {
                pcs.sort_by(|a, b| a.0.total_cmp(&b.0));
                hidden.extend(pcs[cap..].iter().map(|p| p.1));
            }
            // A riding animal goes with its rider; a rider waits hidden for its mount.
            let animals: Vec<u32> = hidden.iter().map(|id| ride::ANIMAL | id).collect();
            hidden.extend(animals);
            hidden.extend(ride::hidden(w));
            let idles = event::idles(w);
            // Placements first (look targets read other actors), then each actor's motion and pose.
            let my_dir = w.net.me.lock().map(|m| m.dir).unwrap_or(0);
            let mut places: HashMap<u32, Place> = w.actors.keys().filter_map(|id| actor_place(w, *id, my_dir).map(|p| (*id, p))).collect();
            // Doors have no model; their schedulers' sounds play at the door's own position (op 0x0b, actor2).
            for (id, e) in w.entities.iter().filter(|(_, e)| e.door != 0 || e.lift.is_some()) {
                places.entry(*id).or_insert(Place { pos: e.pos, forward: forward_from_dir(e.dir), tilt: [0.0; 2], status: 0, ratio: 0.0, size: 0 });
            }
            let look_points: HashMap<u32, Vec3> = w
                .actors
                .iter()
                .filter_map(|(id, a)| match a.look.mode {
                    actmotion::LookMode::Actor(t, k) => attach_point(w, &places, t, k).map(|p| (*id, p)),
                    _ => None,
                })
                .collect();
            // FUN_100d5b10: we look at our target, everyone else at its server face target, only in the statuses
            // that allow it (FUN_10095790), never at doors; the target's point 3, 1.2 higher when it is hidden.
            let mut by_index: HashMap<u16, u32> = w.entities.iter().filter(|(_, e)| e.index != 0).map(|(id, e)| (e.index, *id)).collect();
            by_index.insert(w.my_index, w.my_id);
            let fx_posed = w.effects.posed_actors();
            let face_points: HashMap<u32, Vec3> = w
                .actors
                .keys()
                .filter_map(|id| {
                    let st = places.get(id)?.status;
                    if !matches!(st, 0 | 5 | 0x2f | 0x30 | 0x55 | 0x3f..=0x53) {
                        return None;
                    }
                    let t = if *id == w.my_id { w.target? } else { *by_index.get(&w.entities.get(id).map(|e| e.face).filter(|f| *f != 0)?)? };
                    if t == *id || w.entities.get(&t).is_some_and(|e| e.door != 0) {
                        return None;
                    }
                    let mut q = attach_point(w, &places, t, 3)?;
                    if hidden.contains(&t) {
                        q.y -= 1.2;
                    }
                    Some((*id, q))
                })
                .collect();
            // Foot points that came back from earlier frames: steps (FUN_100d2970).
            if let Some(pts) = w.renderer.as_mut().map(|r| r.points.take()) {
                let (rides, pts): (Vec<_>, Vec<_>) = pts.into_iter().partition(|(k, _)| ride::is_key(*k));
                ride::points(w, rides);
                let (pick, steps): (Vec<_>, Vec<_>) = pts.into_iter().partition(|(k, _)| k >> 63 != 0);
                let mut fresh: HashMap<u32, HashMap<u16, Vec3>> = HashMap::new();
                for (k, p) in pick {
                    fresh.entry((k >> 16) as u32).or_default().insert(k as u16, p);
                }
                w.pick_points.extend(fresh);
                w.pick_points.retain(|id, _| w.pick_rect.contains(id));
                footsteps::steps(w, &places, steps);
            }
            // Door parts animate against the zone's tick clock; the frame's scheduler moves go in first.
            if let Some(r) = w.renderer.as_mut() {
                r.now = w.started.elapsed().as_secs_f32() * 60.0;
            }
            doors::moves(w, &gfx.gpu);
            lifts::moves(w, &gfx.gpu);
            let invis: HashMap<u32, f32> = w.actors.keys().filter_map(|id| invisible_alpha(w, *id).map(|k| (*id, k))).collect();
            // Mog House furniture loaded for this scene goes up.
            if w.renderer.is_some()
                && let Some(l) = w.furniture.ready.take()
            {
                let files = furniture::State::apply(w, l);
                if let Some(r) = w.renderer.as_mut() {
                    r.set_furniture(&gfx.gpu, files);
                }
            }
            if let Some(r) = &mut w.renderer {
                // This frame's copies: a zone going up (more while its indicator hides the world), then the models
                // actors are waiting for.
                use crate::render::upload::{FRAME_MS, LOADING_MS};
                gfx.gpu.begin_uploads(if w.downloading.is_some() { LOADING_MS } else { FRAME_MS });
                perf::timed(|| "uploads".into(), || r.upload_step(&gfx.gpu, &mut encoder));
                if let Some(since) = w.zone_uploading
                    && r.ready(&gfx.gpu)
                {
                    log::info!("zone on the GPU {:.0} ms after it loaded", since.elapsed().as_secs_f32() * 1000.0);
                    w.zone_uploading = None;
                    w.downloading = None;
                }
                // Zone lighting for the Vana'diel time and weather the server gave us.
                if let (Some(zw), Some((t, at))) = (&w.zone_weather, w.clock) {
                    // Op 0x77 sets the hour and weather while an event runs.
                    let ev_time = w.event.as_ref().and_then(|l| l.st.time);
                    let clock = crate::dat::weather::vana_minute(t, at.elapsed().as_secs_f32());
                    let minute = ev_time.and_then(|(h, _)| h).map_or(clock, |h| h as f32 * 60.0);
                    let weather = ev_time.and_then(|(_, x)| x).unwrap_or(w.weather);
                    // A 0x057 change blends the old weather's environment into the new one over its time (retail
                    // weights the two weather objects; the linear weight is inferred).
                    if w.weather_from.is_some_and(|(_, at, secs)| at.elapsed().as_secs_f32() >= secs) {
                        w.weather_from = None;
                    }
                    let blend = w.weather_from.filter(|_| ev_time.and_then(|(_, x)| x).is_none()).map(|(from, at, secs)| (crate::dat::weather::weather_tag(from), (at.elapsed().as_secs_f32() / secs).clamp(0.0, 1.0)));
                    let tag = crate::dat::weather::weather_tag(weather);
                    let mix = |a: &dyn Fn(&str) -> Option<crate::dat::weather::Env>| -> Option<crate::dat::weather::Env> {
                        let now = a(tag)?;
                        Some(match blend.and_then(|(from, t)| Some((a(from)?, t))) {
                            Some((old, t)) => crate::dat::weather::Env::lerp(&old, &now, t),
                            None => now,
                        })
                    };
                    r.lighting = mix(&|t| zw.at(t, minute)).map(|e| crate::dat::weather::Lighting::from_env(&e, minute));
                    let sub_areas = w.sub.weather.iter().flat_map(|s| s.areas.iter());
                    w.area_lights = zw.areas.iter().chain(sub_areas).filter_map(|(&id, a)| Some((id, mix(&|t| a.at(t, minute))?.area_light()))).collect();
                    // Scenery areas (FUN_101889c0 finds the area object by 4cc, the sub-area's after the zone's).
                    let env = |k: &u32| w.sub.weather.as_ref().and_then(|s| s.areas.get(k)).or_else(|| zw.areas.get(k)).and_then(|a| mix(&|t| a.at(t, minute)));
                    let lights = r.area_keys().iter().map(|k| env(k).map(|e| crate::dat::weather::Lighting::from_env(&e, minute))).collect();
                    r.set_area_lights(lights);
                }
                // Op 0x10: the frame before the cut, drawn once offscreen from the camera it had (last frame's
                // poses; its own submit so the zone uniforms keep that camera).
                if let Some(cr) = w.screen.cross.filter(|c| !c.captured) {
                    if let Some(cam) = cr.camera {
                        let fx = self.screen_fx_world.get_or_insert_with(|| crate::render::screenfx::ScreenFx::new(&gfx.gpu, scene_fmt));
                        let target = fx.capture_target(&gfx.gpu, bg);
                        let actors: Vec<&crate::render::ActorGpu> = w.actors.iter().filter(|(id, a)| !a.vanished() && !hidden.contains(id)).filter_map(|(_, a)| a.gpu.as_ref()).collect();
                        let mut enc = gfx.gpu.device.create_command_encoder(&Default::default());
                        r.render(&gfx.gpu, &mut enc, target, &scene_depth, &cam, aspect, &actors);
                        gfx.gpu.queue.submit([enc.finish()]);
                    }
                    if let Some(c) = &mut w.screen.cross {
                        c.captured = true;
                    }
                }
                r.begin_poses();
                let (cview, cproj) = (camera.view(), camera.proj(aspect));
                let eye = camera.eye;
                let mut visible = HashSet::new();
                // Doors: a +-0.6 cube at the door (FUN_100ac8e0's class), tested like the rest.
                let cube = box_corners([-0.6, 0.6, 0.6, -0.6, 0.6, -0.6]);
                // The Mog House door (entity +0x12c bit 17, vtable +0x148) is never culled.
                if w.entities.contains_key(&myroom::DOOR_ID) {
                    visible.insert(myroom::DOOR_ID);
                }
                for (id, e) in w.entities.iter().filter(|(_, e)| e.door != 0) {
                    if let Some((rect, z)) = box_on_screen(cview, cproj, glam::Mat4::from_translation(e.pos), &cube) {
                        visible.insert(*id);
                        r.occl.query(*id, rect, z);
                    }
                }
                let in_event = w.event.is_some() || w.event_pending.is_some();
                // Scheduler ops 0x40..0x47: shine images go up once; each actor's overlay this frame.
                let mut shines: HashMap<u32, crate::render::Shine> = HashMap::new();
                for (id, (lib, node), u, v, tf) in w.effects.shines() {
                    let key = (w.effects.reg.libs[lib].file_id, node);
                    if !r.has_shine(key)
                        && let Some(t) = w.effects.reg.libs[lib].texture_at(node)
                    {
                        r.add_shine(&gfx.gpu, key, &w.effects.reg.libs[lib].textures[t]);
                    }
                    let c = |sh: u32| ((tf >> sh) & 0xff) as f32 / 255.0;
                    shines.insert(id, crate::render::Shine { key, u, v, tf: [c(16), c(8), c(0), c(24)] });
                }
                for (id, a) in w.actors.iter_mut() {
                    let Some(ch) = a.character.clone() else { continue };
                    let Some(&Place { pos, forward, tilt, status, ratio, size }) = places.get(id) else { continue };
                    if a.gpu.is_none() {
                        // Not drawn until its model is on the GPU (a few new models a frame).
                        let Some(g) = perf::timed(|| format!("model {id:#x} {} ({} textures)", a.key, ch.textures.len()), || r.skinned_actor(&gfx.gpu, &ch)) else { continue };
                        a.gpu = Some(g);
                        a.top_key.clear();
                    }
                    // A one-off combat motion wins; it ends after its length and blends back.
                    if a.oneshot.as_ref().is_some_and(|o| o.ticks >= o.len) {
                        a.oneshot = None;
                    }
                    let extra = a.extra.clone();
                    let cache = std::cell::RefCell::new(std::mem::take(&mut a.find_cache));
                    let has = |n: &str| !actmotion::find_cached(&mut cache.borrow_mut(), &extra, &ch, n).is_empty();
                    // Motion and rate as FFXiMain picks them (FUN_100c6b70); frame += ticks * speed * 0.5.
                    // Then a scheduler's motion (event / weapon draw) over the status motion.
                    let (name, rate, once, blend) = match (&a.oneshot, a.play.as_ref().and_then(|p| p.current())) {
                        (Some(o), _) => (o.name.clone(), o.speed, true, crate::motion::BLEND_TICKS),
                        (None, Some(m)) => {
                            let rate = actmotion::op_rate(actmotion::find_cached(&mut cache.borrow_mut(), &extra, &ch, &m.name).first().copied(), m.ticks as f32);
                            (m.name.clone(), rate, m.count == 1, m.blend.max(1) as f32)
                        }
                        // Op 0x5e / 0x6b: the event's idle motion while the actor stands.
                        (None, None) if ratio < 1e-3 && idles.get(id).is_some_and(|n| has(n)) => (idles[id].clone(), 1.0, false, crate::motion::BLEND_TICKS),
                        // FUN_100c8930 (LAB_100c8dde): while the jump timer runs, "jmp" at rate 1 ("jmp*" engaged, "jmp "
                        // otherwise; ours takes the prefix).
                        (None, None) if a.jump >= 0.0 && matches!(status, 0 | 1) => ("jmp".to_string(), 1.0, false, crate::motion::BLEND_TICKS),
                        (None, None) => {
                            let (n, r) = crate::motion::choose(&a.motion_names.unwrap_or(ch.motions), status, ratio);
                            (n.to_string(), r * a.rate_scale, false, crate::motion::BLEND_TICKS)
                        }
                    };
                    if name != a.motion || (once && a.oneshot.as_ref().is_some_and(|o| o.ticks == 0.0)) {
                        log::debug!(target: "motion", "actor {id:#x}: {} -> {name} at {:.1} (oneshot {}, play {}, status {status}, ratio {ratio:.2})", a.motion, a.ticks, a.oneshot.is_some(), a.play.is_some());
                        if !a.motion.is_empty() {
                            a.prev = Some((std::mem::take(&mut a.motion), a.ticks, a.once));
                            a.blend = 0.0;
                        }
                        a.blend_len = blend;
                        a.motion = name.clone();
                        a.ticks = 0.0;
                    }
                    a.once = once;
                    a.ticks += dt * 60.0 * rate;
                    if a.jump >= 0.0 {
                        a.jump -= dt * 60.0;
                    }
                    // One-shots end by real ticks: their motion runs at the op's stretch rate.
                    if let Some(o) = &mut a.oneshot {
                        o.ticks += dt * 60.0;
                    }
                    // Fall back to idle when a model lacks a motion; chocobos idle with chi.
                    let frames_of = |motion: &str, ticks: f32, once: bool| -> Vec<_> {
                        [motion, ch.motions.idle, "idl", "chi"]
                            .iter()
                            .map(|n| actmotion::find_cached(&mut cache.borrow_mut(), &extra, &ch, n))
                            .find(|a| !a.is_empty())
                            .unwrap_or_default()
                            .into_iter()
                            .map(|x| {
                                let f = ticks * x.speed * 0.5;
                                (x, if once { f.min(x.frames.max(1) as f32 - 1.0) } else { crate::motion::loop_frame(f, x.frames) })
                            })
                            .collect()
                    };
                    // FUN_1001ad00's queue replaces only the regions a new motion keys: under a one-shot or scheduler motion
                    // the status motion goes on (an Elvaan's moving swing amf1 keys the upper body over the run), so it
                    // goes first and the motion's tracks win where it has them.
                    let (base, base_rate) = crate::motion::choose(&a.motion_names.unwrap_or(ch.motions), status, ratio);
                    a.base_ticks += dt * 60.0 * base_rate * a.rate_scale;
                    let mut anims = if name != base && (a.oneshot.is_some() || a.play.is_some()) { frames_of(base, a.base_ticks, false) } else { Vec::new() };
                    anims.extend(frames_of(&name, a.ticks, once));
                    // The mouth motion of a spoken line goes over the body motion (its face tracks win).
                    if let Some(m) = &mut a.mouth
                        && let Some(mt) = m.ticks
                    {
                        // FUN_100ce0e0: the entity's face motion, else 'mou6'.
                        let mouth: Vec<_> = [a.mouth_name, "mou6"].iter().map(|n| actmotion::find_cached(&mut cache.borrow_mut(), &extra, &ch, n)).find(|v| !v.is_empty()).unwrap_or_default();
                        let mut playing = false;
                        for x in mouth {
                            let f = mt * x.speed * 0.5;
                            if f < x.frames.max(1) as f32 - 1.0 {
                                anims.push((x, f));
                                playing = true;
                            }
                        }
                        if !playing {
                            m.ticks = None;
                        }
                    }
                    // FUN_100cdf90: every rand() % 480 ticks the eyes blink (eye3, region 3) unless op 0xb5 holds them.
                    a.blink_wait -= dt * 60.0;
                    if a.blink_wait < 0.0 {
                        a.blink_wait = (actmotion::crand() % 480) as f32;
                        if !w.effects.no_blink.contains(id) {
                            a.blink = Some(0.0);
                        }
                    }
                    if let Some(bt) = a.blink.as_mut() {
                        *bt += dt * 60.0;
                        let eyes = actmotion::find_cached(&mut cache.borrow_mut(), &extra, &ch, "eye3");
                        let mut playing = false;
                        for x in eyes {
                            let f = *bt * x.speed * 0.5;
                            if f < x.frames.max(1) as f32 - 1.0 {
                                anims.push((x, f));
                                playing = true;
                            }
                        }
                        if !playing {
                            a.blink = None;
                        }
                    }
                    a.blend += dt * 60.0;
                    if a.blend >= a.blend_len {
                        a.prev = None;
                    }
                    // Motions are evaluated and skinned on the GPU (render/anim.rs, vs_skinned).
                    let from = a.prev.as_ref().map(|(old, old_ticks, old_once)| frames_of(old, *old_ticks, *old_once)).unwrap_or_default();
                    // The reaction overlay, its first animation found; dropped once faded out.
                    let overlay = a.overlay.as_mut().and_then(|o| {
                        o.ticks += dt * 60.0;
                        let x = actmotion::find_cached(&mut cache.borrow_mut(), &extra, &ch, &o.name).into_iter().next()?;
                        o.sample(x.frames as u32, x.speed).map(|(f, wt)| (x, f, wt))
                    });
                    if overlay.is_none() {
                        a.overlay = None;
                    }
                    a.find_cache = cache.take();
                    let t = a.blend / a.blend_len;
                    a.scale = a.event_scale.unwrap_or_else(|| ch.scale(size));
                    // The rest pose's mesh top, once (stub models, plates without attach points).
                    if a.top_key.is_empty() {
                        a.rest = crate::dat::character::pose(&ch.skeleton, &[]);
                        a.top = crate::render::character::skinned_top(&ch, &a.rest) * a.scale;
                        a.top_key = "rest".into();
                    }
                    // CPU bones only where they are read: the locked camera (player bone 13, target bone 5), first
                    // person, and the attach points of actors with live effects.
                    let pose_now = || if from.is_empty() { crate::dat::character::pose(&ch.skeleton, &anims) } else { crate::render::character::pose_blend(&ch, &from, &anims, t) };
                    if (w.locked && (*id == w.my_id || w.target == Some(*id))) || (w.camst.first && *id == w.my_id) || fx_posed.contains(id) {
                        a.bones = pose_now();
                    } else {
                        a.bones.clear();
                    }
                    // FUN_100cbde0: leg mode 1 / 2 models and dead actors follow the ground (FUN_100d54c0); a riding animal
                    // by its rider's status (box 1) and ground normal.
                    let rider = (*id & ride::ANIMAL != 0).then_some(*id & !ride::ANIMAL);
                    let gate_status = rider.and_then(|r| places.get(&r)).map_or(status, |p| p.status);
                    a.tilt_rot = None;
                    if tilt::eligible(ch.cib[0], gate_status) {
                        // Ours: the player's walk normal, else the face under the position each frame (the rider's for an
                        // animal at its place) *(retail skips far moving actors, up to 121 frames)*.
                        let n = if rider.unwrap_or(*id) == w.my_id {
                            w.ground.as_ref().map(|g| g.player_normal())
                        } else {
                            w.ground.as_ref().and_then(|g| g.floor_normal(pos)).map(|n| -n)
                        };
                        if let Some(n) = n {
                            tilt::set_normal(&mut a.ground_tilt, n);
                        }
                        // FUN_100a4d10: info[0] == info[2] == 2 (behemoth, adamantoise, morbol ...) pitch with the ground under
                        // the front feet (points 10 / 11) and the rear feet (8 / 9), each snapped *(inf: the rest pose through
                        // last frame's matrix for retail's posed points)*.
                        if ch.cib[0] == 2 && ch.cib[2] == 2
                            && let Some(g) = w.ground.as_ref()
                        {
                            let m = actor_frame(&Place { pos, forward, tilt, status, ratio, size }, a) * glam::Mat4::from_scale(Vec3::splat(a.scale));
                            let pt = |k: usize| ch.skeleton.points.get(k).and_then(|(b, off)| a.rest.get(*b as usize).map(|(q, t)| m.transform_point3(*t + *q * *off)));
                            let snapped = |p: Vec3| g.snap(p.x, p.z, p.y).map(|y| Vec3::new(p.x, y, p.z));
                            if let (Some(p10), Some(p11), Some(p8), Some(p9)) = (pt(10), pt(11), pt(8), pt(9))
                                && let (Some(fa), Some(fb)) = (snapped((p10 + p11) * 0.5), snapped((p8 + p9) * 0.5))
                                && let Some(n) = tilt::feet_normal(fa, fb)
                            {
                                tilt::set_normal(&mut a.ground_tilt, n);
                            }
                        }
                        let zf = plain_frame(pos, forward, tilt).z_axis.truncate();
                        let bx = ch.skeleton.boxes.get(box_index(gate_status)).or(ch.skeleton.boxes.first()).copied().unwrap_or([f32::NAN; 6]);
                        a.tilt_rot = Some(a.ground_tilt.step(zf, Vec3::splat(a.scale), bx, tilt::rate(dt * 60.0)));
                    }
                    let frame = actor_frame(&Place { pos, forward, tilt, status, ratio, size }, a);
                    let model = frame * glam::Mat4::from_scale(Vec3::splat(a.scale));
                    // FUN_100cd040 (camera_collision_spec.md 2.2): the eye near posed point 12 fades the actor (not the
                    // player in first person, who fades on its own). A CPU pose only when the rest pose is close.
                    let cam_fade = if *id == w.my_id && w.camst.first {
                        1.0
                    } else {
                        let (b12, off12) = ch.skeleton.points.get(12).copied().unwrap_or((0, Vec3::ZERO));
                        let at = |bones: &[(glam::Quat, Vec3)]| model.transform_point3(bones.get(b12 as usize).map_or(Vec3::ZERO, |(q, t)| *t + *q * off12));
                        let rest = at(&a.rest);
                        if Vec3::new(rest.x - eye.x, 0.0, rest.z - eye.z).length() > 0.5 + 2.0 * a.scale {
                            1.0
                        } else if a.bones.is_empty() {
                            near_camera_fade(at(&pose_now()), eye)
                        } else {
                            near_camera_fade(at(&a.bones), eye)
                        }
                    };
                    // FUN_100d5b10: an event's look, else the target / face target; eased and weighted in world space.
                    let point = |k: usize| ch.skeleton.points.get(k).copied().unwrap_or((0, Vec3::ZERO));
                    let p3 = model.transform_point3(a.rest.get(point(3).0 as usize).map_or(Vec3::ZERO, |(q, t)| *t + *q * point(3).1));
                    let target = match a.look.mode {
                        actmotion::LookMode::Actor(..) => look_points.get(id).copied(),
                        actmotion::LookMode::Facing(pitch, yaw) => {
                            let (p, y) = (pitch as f32 * TAU_4096, yaw as f32 * TAU_4096);
                            let head = a.rest.get(point(7).0 as usize).map_or(Vec3::ZERO, |b| b.1);
                            Some(model.transform_point3(head + Vec3::new(p.cos() * y.cos(), -p.sin(), -p.cos() * y.sin()) * 20.0))
                        }
                        actmotion::LookMode::Off => face_points.get(id).copied(),
                    };
                    // FUN_100d5b10: op 0x59's look percent replaces the 0.04 step during an event.
                    let rate = if in_event && a.look_pct != 0 { a.look_pct as f32 * 0.01 } else { 1.0 };
                    a.look.step(frame, p3, target, dt * 60.0, rate);
                    // One bone (the head) when riding or sitting; point 4 a unit higher on a chocobo or a mount.
                    let look = (a.look.weight > 0.0).then(|| crate::render::anim::HeadLook {
                        bones: [3usize, 7].map(|k| ch.skeleton.points.get(k).map(|p| p.0)),
                        two: !matches!(status, 5 | 0x55 | 0x2f | 0x30 | 0x3f..=0x53),
                        weight: a.look.weight,
                        point: model.inverse().transform_point3(a.look.point.unwrap_or(p3)),
                        p3: point(3).1,
                        p4: point(4),
                        drop: if matches!(status, 5 | 0x55) { 1.0 / a.scale.max(1e-3) } else { 0.0 },
                        limits: ch.skeleton.look_limits,
                    });
                    let drawn = !a.vanished() && !hidden.contains(id) && !(*id == w.my_id && w.camst.self_alpha <= 0.0);
                    // Tests 1-5 of FUN_100c63d0 / FUN_100cbde0: a box, faded in, in front and on screen.
                    if drawn
                        && a.alpha > 0.0
                        && let Some(b) = ch.skeleton.boxes.get(box_index(status)).or(ch.skeleton.boxes.first())
                        && let Some((rect, z)) = box_on_screen(cview, cproj, model, &box_corners(*b))
                    {
                        visible.insert(*id);
                        if *id != w.my_id {
                            r.occl.query(*id, rect, z);
                        }
                    }
                    let g = a.gpu.as_mut().unwrap();
                    // FUN_100cbde0: k = model alpha x the near-camera fade, drawn as 1 from 0.8 up, else 1.25 k; then
                    // the first-person fade.
                    let k = (a.alpha / actmotion::OPAQUE).max(0.0) * cam_fade;
                    let k = if k >= 0.8 { 1.0 } else { k * 1.25 } * a.appear;
                    g.alpha = (k * if *id == w.my_id { w.camst.self_alpha } else { 1.0 }).min(1.0);
                    if let Some(k) = invis.get(id) {
                        g.alpha = g.alpha.min(*k);
                    }
                    g.tint = a.rgb.map(|c| c.max(0.0) / actmotion::OPAQUE);
                    for n in 0..16 {
                        if a.wep_show_at[n].is_some_and(|t| Instant::now() >= t) {
                            (a.wep_hide[n][0], a.wep_show_at[n]) = (false, None);
                        }
                    }
                    g.hidden_weps = (0..16).filter(|&n| a.wep_hide[n].iter().any(|&h| h)).fold(0, |m, n| m | 1 << n);
                    g.shine = shines.get(id).copied();
                    // The flash runs only while the actor is drawn, in 60 Hz ticks whatever the frame rate.
                    g.flash = if drawn { a.flash.step(w.target == Some(*id), dt * 60.0) } else { flash::NONE };
                    // The floor's lights (retail copies them at the ground probe, every 121 updates).
                    g.lights = w.ground.as_ref().map_or([0; 4], |gr| gr.lights(pos));
                    g.area = w.ground.as_ref().and_then(|gr| gr.area(pos)).unwrap_or(0);
                    if drawn {
                        let files: Vec<_> = extra.iter().map(|(_, f)| f.clone()).collect();
                        r.pose_actor_gpu(g, &ch, &files, &anims, if a.prev.is_some() { &from } else { &[] }, t, model, look, overlay);
                        counts.posed += 1;
                        // FUN_100c9330: a mounted rider's point 0x6b and the mount's saddle point, for the offset.
                        if let Some((key, k)) = ride::query(&w.rides, *id)
                            && let Some(&(bone, off)) = ch.skeleton.points.get(k)
                        {
                            r.query_point(g, key, bone as u16, off);
                        }
                        // The pick's fine test: the geometry bones and attach points 1 / 3 / 13 / 15 of an actor whose rect
                        // held the pointer, read back a frame or two late.
                        if w.pick_rect.contains(id) {
                            let bones = a.pick_bones.get_or_insert_with(|| {
                                let f = ch.geometry_bones();
                                (1..f.len()).filter(|&b| f[b] && f[ch.skeleton.bones[b].parent]).map(|b| b as u16).collect()
                            });
                            let mut wanted: Vec<u16> = bones.iter().flat_map(|&b| [b, ch.skeleton.bones[b as usize].parent as u16]).collect();
                            wanted.sort_unstable();
                            wanted.dedup();
                            for b in wanted {
                                r.query_point(g, pick_key(*id, b), b, Vec3::ZERO);
                            }
                            for k in [1usize, 3, 13, 15] {
                                if let Some(&(bone, off)) = ch.skeleton.points.get(k) {
                                    r.query_point(g, pick_key(*id, 0x8000 | k as u16), bone as u16, off);
                                }
                            }
                        }
                        if footsteps::wanted(&ch, status, pos, camera.eye) {
                            for (slot, &k) in footsteps::POINTS.iter().enumerate().take(if ch.cib[0] == 2 { 4 } else { 2 }) {
                                if let Some(&(bone, off)) = ch.skeleton.points.get(k) {
                                    r.query_point(g, footsteps::key(*id, slot), bone as u16, off);
                                }
                            }
                        }
                    }
                }
                // Test 6: the occlusion results, a frame or two late, drop actors hidden behind scenery.
                r.occl.results.retain(|id, _| visible.contains(id));
                visible.retain(|id| r.occl.results.get(id) != Some(&false));
                w.visible = visible;
                r.upload_poses(&gfx.gpu, &mut encoder);
                // FUN_100cbde0: a drawn alpha under 0.0001 is not drawn (the near-camera fade, first person).
                let actors: Vec<&crate::render::ActorGpu> = w.actors.iter().filter(|(id, a)| !a.vanished() && !hidden.contains(id)).filter_map(|(_, a)| a.gpu.as_ref()).filter(|g| g.alpha >= 1e-4).collect();
                counts = perf::Counts { entities: w.entities.len(), actors: w.actors.len(), posed: counts.posed, drawn: actors.len(), zone_draws: r.draw_count(), ui_vertices: 0 };
                m_actors = Instant::now();
                // Mips of new textures (models that came this frame, a zone going up) under the frame budget.
                let budget = if w.zone_uploading.is_some() || w.downloading.is_some() { crate::render::MIP_BUDGET } else { crate::render::MIP_BUDGET_WORLD };
                perf::timed(|| "mips".into(), || gfx.gpu.run_mips(&mut encoder, budget));
                perf::timed(|| "zone render".into(), || r.render(&gfx.gpu, &mut encoder, &scene_raw, &scene_depth, &camera, aspect, &actors));
                r.occl.run(&gfx.gpu, &mut encoder, &scene_depth);
                // Effect generators: CPU emission, the GPU update pass, then the elements over the scene.
                // Its pipelines are made while the zone still goes up (behind the loading indicator), not on the
                // first shown frame.
                let eg = self.effects_gpu.get_or_insert_with(|| crate::render::effects::EffectsGpu::new(&gfx.gpu, scene_fmt));
                if !eg.has_dmgnum()
                    && let Some((lib, image)) = self.game_ui.as_ref().and_then(|g| g.dmgnum())
                {
                    eg.set_dmgnum(&gfx.gpu, lib, image);
                }
                if !r.ready(&gfx.gpu) {
                    eg.upload_pending(&gfx.gpu, &mut w.effects);
                }
                if r.ready(&gfx.gpu) {
                    let view = camera.view();
                    let mut view_rot = view;
                    view_rot.w_axis = glam::Vec4::W;
                    let minute = w.clock.map(|(t, at)| crate::dat::weather::vana_minute(t, at.elapsed().as_secs_f32())).unwrap_or(720.0);
                    let f = crate::fx::Frame {
                        dt: (dt * 60.0).max(1e-3),
                        seconds: dt,
                        dayfrac: minute / 1440.0,
                        view,
                        view_rot,
                        eye: camera.eye,
                        draw_distance: 2000.0,
                        // Op 0x8f: the actor's area env, else the zone; areas are not loaded *(the zone's)*.
                        area_light: r.lighting.map_or(Vec3::ONE, |l| l.area),
                        focal: 192.0 / (camera.fov_y * 0.5).tan(),
                        // FUN_10184dc0: ((Vana day of now + 12 h) / 7) % 12 (one Vana minute = 2.4 s).
                        moon_phase: w.clock.map_or(0, |(t, at)| ((((t as f64 + at.elapsed().as_secs_f64()) / 2.4 + 720.0) / 1440.0) as u64 / 7 % 12) as u32),
                        weekday: w.clock.map_or(0, |(t, at)| (((t as f64 + at.elapsed().as_secs_f64()) / 2.4 / 1440.0) as u64 & 7) as u32),
                    };
                    let party: Vec<u32> = std::iter::once(w.my_id).chain(w.party.party().map(|m| m.id).filter(|&id| id != w.my_id)).collect();
                    let fa = FxActors { actors: &w.actors, places: &places, contacts: &w.contacts, entities: &w.entities, me: w.my_id, me_moving: w.moving, pet: w.my_pet, party, my_index: w.my_index, areas: &w.area_lights, ground: w.ground.as_ref(), bells: w.game.get(0x99) & 2 != 0, furniture: &w.furniture.pieces };
                    if let Some(g) = &w.ground {
                        w.effects.set_covered(g.covered(camera.eye));
                    }
                    if let Some(a) = &self.audio {
                        w.effects.set_alive(a.alive());
                    }
                    w.effects.filter = (w.game.get(0x92) as u32, matches!(w.my_motion.status, 1 | 3));
                    w.effects.reg.aspect = aspect;
                    perf::timed(|| "effects".into(), || w.effects.tick(&f, &fa));
                    w.ambient = w.effects.ambient(minute);
                    // Effect sounds (fx/sound.rs) to the sound thread; without audio they are dropped.
                    for c in std::mem::take(&mut w.effects.sound_cmds) {
                        let Some(a) = &mut self.audio else { break };
                        play_fx_sound(a, c);
                    }
                    eg.set_lights(&r.lights);
                    perf::timed(|| "effects upload".into(), || eg.update(&gfx.gpu, &mut encoder, &mut w.effects, &f));
                    let (fog_color, fog) = r.lighting.as_ref().map_or(([0.0; 3], [150.0, 600.0]), |l| (l.fog.to_array(), [l.fog_start, l.fog_end]));
                    eg.draw(&gfx.gpu, &mut encoder, (&scene_raw, &scene_depth, bg), Some(&scene_tex), &w.effects, (camera.proj(aspect), camera.view(), camera.eye), [crate::render::NEAR, crate::render::FAR], fog_color, fog);
                }
                // Event crossfade, the effect flash and feedback blur, and the fades over the scene, under the 2D layer;
                // the zoning fade as a multiply too.
                let (cross, m) = (w.screen.cross_alpha(), w.screen.multiply().map(|c| c * (1.0 - w.fade)));
                let (flash, blur) = (w.screen.flash(), w.screen.blur());
                if cross.is_some() || flash.is_some() || blur.is_some() || m.iter().any(|c| (c - 1.0).abs() > 0.005) {
                    let fx = self.screen_fx_world.get_or_insert_with(|| crate::render::screenfx::ScreenFx::new(&gfx.gpu, scene_fmt));
                    fx.draw_all(&gfx.gpu, &mut encoder, &scene_raw, Some(&scene_tex), bg, cross, flash, blur, m);
                }
                w.last_camera = Some(camera);
                drew_world = true;
            }
        }

        if matches!(&self.phase, Phase::Characters { screen: LobbyScreen::Create, .. })
            && let Some(cr) = &mut self.creator
        {
            cr.advance(dt);
            let nation_step = self.creator_step >= 6;
            cr.show_dancer = !nation_step;
            match &mut self.nation.fly {
                Some((_, fly)) if nation_step => {
                    fly.advance(dt);
                    if fly.done {
                        fly.restart();
                    }
                    fly.render(&gfx.gpu, gfx.config.format, &mut encoder, &scene_view, &scene_depth, bg, aspect);
                }
                _ => {
                    let alive = self.audio.as_ref().map(|a| a.alive());
                    cr.render(&gfx.gpu, gfx.config.format, &mut encoder, (&scene_view, Some(&scene_tex), bg), &scene_depth, aspect, alive);
                    for c in cr.effects.as_mut().map(|fx| fx.take_sounds()).unwrap_or_default() {
                        let Some(a) = &mut self.audio else { break };
                        play_fx_sound(a, c);
                    }
                }
            }
            drew_world = true;
        }
        if matches!(&self.phase, Phase::Characters { screen: LobbyScreen::Select | LobbyScreen::Delete, .. })
            && let Some(sel) = &mut self.select
        {
            sel.advance(dt, self.select_ui.cursor);
            let alive = self.audio.as_ref().map(|a| a.alive());
            sel.render(&gfx.gpu, gfx.config.format, &mut encoder, (&scene_view, Some(&scene_tex), bg), &scene_depth, aspect, alive);
            for c in sel.effects().map(|fx| fx.take_sounds()).unwrap_or_default() {
                let Some(a) = &mut self.audio else { break };
                play_fx_sound(a, c);
            }
            drew_world = true;
        }
        if self.title_backdrop()
            && let Some(fly) = &mut self.flyover
        {
            fly.advance(dt);
            drew_world = fly.render(&gfx.gpu, gfx.config.format, &mut encoder, &scene_view, &scene_depth, bg, aspect) || drew_world;
        }
        // The lobby scene fade (a multiply over the drawn scene) and the 2D layer's level.
        let (ui_level, scene_level) = match &self.phase {
            // While the indicator is up it alone is drawn, at full level. Out of the lobby the empty screen is
            // black, not the lobby's 0x808080 (scene level 0 clears to black).
            Phase::World(w) if w.downloading.is_some() => (1.0, 0.0),
            Phase::World(w) => (1.0 - w.fade, 0.0),
            _ => (self.lobby_fade.ui, self.lobby_fade.scene),
        };
        if !matches!(self.phase, Phase::World(_)) && drew_world && scene_level < 0.995 {
            let fx = self.screen_fx.get_or_insert_with(|| crate::render::screenfx::ScreenFx::new(&gfx.gpu, gfx.config.format));
            fx.draw(&gfx.gpu, &mut encoder, &scene_view, bg, None, [scene_level; 3]);
        }
        if drew_world
            && scaled
            && let Some(sc) = &self.scene_scale
        {
            sc.blit(&gfx.gpu, &mut encoder, &view, size, gamma);
        }
        self.window_size = size;
        self.mouse_kind(size);
        // Config 0xa4 (Window Settings): windows open without the zoom (N = 0, FUN_10199500).
        let no_zoom = match &self.phase {
            Phase::World(w) => w.game.get(0xa4),
            _ => self.cfg.game.global.get(0xa4),
        } != 0;
        if self.ui_drawn
            && let Some(lu) = &mut self.game_ui
        {
            lu.open_ticks = if no_zoom { 0.0 } else { 25.0 };
            lu.end_frame(dt);
            if self.cfg.tool("0021") == 0
                && self.cursor_in
                && let Some((x, y)) = self.last_cursor
            {
                // The sprite is in UI pixels: undo the UI stretch so sizes are in screen pixels. Auto grows only the
                // sprite; a cursor pack's images stay at their own size, as the hardware cursor shows them.
                let s = self.cfg.cursor_scale(size[1]) * self.ui_k.1;
                let fs = if self.cfg.cursor_size == 0 { 1.0 } else { self.cfg.cursor_size as f32 / 100.0 } * self.ui_k.1;
                let dy = if let Phase::World(w) = &self.phase { w.ptr_dy } else { 0.0 };
                self.mouse.draw(lu, self.index.as_ref(), x as f32 * self.ui_k.0, y as f32 * self.ui_k.1 + dy, dt, s, fs);
            }
            counts.ui_vertices = lu.vertex_count();
            lu.scene_depth = drew_world.then(|| scene_depth.clone());
            perf::timed(|| "ui render".into(), || lu.render(&gfx.gpu, gfx.config.format, &mut encoder, &view, self.cfg.ui_size(size), size, drew_world, ui_level, scene_level));
            drew_world = true;
        }

        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: size, pixels_per_point: output.pixels_per_point };
        if !output.textures_delta.set.is_empty() {
            perf::note(|| format!("egui textures {}", output.textures_delta.set.len()));
        }
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                gfx.egui_renderer.update_texture(&gfx.gpu.device, &gfx.gpu.queue, *id, delta);
            }
        }
        let extra = gfx.egui_renderer.update_buffers(&gfx.gpu.device, &gfx.gpu.queue, &mut encoder, &jobs, &screen);
        {
            let load = if drew_world { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color { r: 0.05, g: 0.06, b: 0.1, a: 1.0 }) };
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load, store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            gfx.egui_renderer.render(&mut pass, &jobs, &screen);
        }
        for id in &output.textures_delta.free {
            gfx.egui_renderer.free_texture(id);
        }
        let row = (size[0] * 4).div_ceil(256) * 256;
        let capture = (shoot && gfx.config.usage.contains(wgpu::TextureUsages::COPY_SRC)).then(|| {
            let buf = gfx.gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("screenshot"),
                size: (row * size[1]) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                frame.texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(size[1]) } },
                wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            );
            buf
        });
        perf::timed(|| "submit".into(), || gfx.gpu.queue.submit(extra.into_iter().chain([encoder.finish()])));
        if let (Some(eg), Phase::World(w)) = (&mut self.effects_gpu, &mut self.phase) {
            eg.after_submit(&gfx.gpu, &mut w.effects);
        }
        if let Some(fx) = self.select.as_mut().and_then(|s| s.effects()) {
            fx.after_submit(&gfx.gpu);
        }
        if let Some(fx) = self.creator.as_mut().and_then(|c| c.effects.as_mut()) {
            fx.after_submit(&gfx.gpu);
        }
        if let Phase::World(w) = &mut self.phase
            && let Some(r) = &mut w.renderer
        {
            r.points.after_submit(&gfx.gpu);
            r.occl.after_submit(&gfx.gpu);
        }
        if let (Some(buf), Some(path)) = (capture, script_shot.or_else(|| self.drive.screenshot.clone())) {
            let saved = save_capture(&gfx.gpu, &buf, size, row, gfx.config.format, &path);
            match &saved {
                Err(e) => log::error!("screenshot failed: {e:#}"),
                Ok(()) => log::info!("screenshot written to {}", path.display()),
            }
            // FUN_101d0c70 states 3 / 4: group 9 0x42 on success, SE "0004" (and group 7 0x8d, not loaded) on failure.
            if game_shot
                && let Phase::World(w) = &mut self.phase
                && let Some(bt) = w.battle.clone()
            {
                let line = if saved.is_ok() { bt.group9(0x42) } else { None };
                if saved.is_err() {
                    w.sounds.push(4);
                }
                if let Some(l) = line {
                    w.chat.push(l.replace("%s", "").trim().to_string());
                }
            }
        }
        let m_encode = Instant::now();
        perf::timed(|| "present".into(), || gfx.gpu.queue.present(frame));
        self.perf.counts = counts;
        self.perf.frame(t0, [m_update, m_ui, m_acquire, m_actors, m_encode, Instant::now()]);
        // The frame limit paces every screen from this frame's start; game time runs on dt either way.
        if self.frame_limit > 0 {
            self.next_frame = Some(t0 + std::time::Duration::from_secs_f64(1.0 / self.frame_limit as f64));
        } else {
            gfx.window.request_redraw();
        }
        self.gfx = Some(gfx);
    }
}

impl ApplicationHandler for App {
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        use winit::event_loop::ControlFlow;
        match self.next_frame {
            Some(t) if Instant::now() < t => event_loop.set_control_flow(ControlFlow::WaitUntil(t)),
            Some(_) => {
                self.next_frame = None;
                event_loop.set_control_flow(ControlFlow::Wait);
                // A hidden window gets no redraw events on Windows (no WM_PAINT): drive the frames until it shows.
                if !self.shown && self.gfx.is_some() {
                    self.redraw();
                    if self.exit_requested {
                        event_loop.exit();
                    }
                    // The frame it scheduled needs the timer set again.
                    if let Some(t) = self.next_frame {
                        event_loop.set_control_flow(ControlFlow::WaitUntil(t));
                    }
                } else if let Some(g) = &self.gfx {
                    g.window.request_redraw();
                }
            }
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        // Hidden (and on Wayland unmapped, nothing being presented) until the title can draw. Driven test runs
        // don't ask for focus and say so in the title, so they never take the user's keyboard.
        let title = if self.drive.driven() { "FINAL FANTASY XI [test]" } else { "FINAL FANTASY XI" };
        // No game files yet: the first-start window is a fixed size, whatever the config says.
        let bootstrap = firstrun::needed(&self.cfg.ffxi_dir());
        let [ww, wh] = if bootstrap { firstrun::WINDOW } else { self.cfg.window_size() };
        // Test runs are always windowed.
        let mode = if self.drive.driven() || bootstrap { 1 } else { self.cfg.window_mode() };
        let attrs = Window::default_attributes()
            .with_title(title)
            .with_resizable(!bootstrap)
            .with_inner_size(winit::dpi::PhysicalSize::new(ww, wh))
            .with_fullscreen(fullscreen(mode, [ww, wh], pick_monitor(&self.cfg.monitor, event_loop.primary_monitor(), event_loop.available_monitors(), [ww, wh])))
            .with_decorations(mode == 1)
            .with_visible(false)
            .with_active(!self.drive.driven())
            .with_window_icon(window_icon());
        // Wayland ignores window icons: the compositor finds ours through the app id and vana-client.desktop.
        #[cfg(target_os = "linux")]
        let attrs = {
            use winit::platform::{wayland::WindowAttributesExtWayland, x11::WindowAttributesExtX11};
            let attrs = WindowAttributesExtWayland::with_name(attrs, "vana-client", "vana-client");
            WindowAttributesExtX11::with_name(attrs, "vana-client", "vana-client")
        };
        let window = Arc::new(event_loop.create_window(attrs).expect("creating the window"));
        if let Some(m) = window.current_monitor() {
            crate::gamecfg::set_desktop(m.size().width, m.size().height);
        }
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(Box::new(event_loop.owned_display_handle())));
        let surface = instance.create_surface(window.clone()).expect("creating the surface");
        self.gpus = crate::render::adapter_names(&instance, Some(&surface));
        let gpu = Gpu::with_adapter(instance, Some(&surface), &self.cfg.gpu).expect("opening the GPU");
        let size = window.inner_size();
        // A software adapter (a VM without a GPU): the first time, the scene draws at half the window's size.
        if gpu.adapter.get_info().device_type == wgpu::DeviceType::Cpu && !self.cfg.software_gpu_seen {
            self.cfg.software_gpu_seen = true;
            if self.cfg.tool.get("0003").is_none() && self.cfg.tool.get("0004").is_none() {
                self.cfg.set_tool("0003", (size.width / 2).max(320));
                self.cfg.set_tool("0004", (size.height / 2).max(240));
                log::info!("software GPU: background set to {}x{}", self.cfg.tool("0003"), self.cfg.tool("0004"));
            }
            // Anisotropic filtering costs a software rasterizer the most per texel.
            if self.cfg.anisotropy == Config::default().anisotropy {
                self.cfg.anisotropy = 1;
                crate::render::ANISOTROPY.store(1, std::sync::atomic::Ordering::Relaxed);
            }
            if let Err(e) = self.cfg.save() {
                log::warn!("saving the config: {e}");
            }
        }
        let mut config = surface.get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1)).expect("surface unsupported by the adapter");
        let caps = surface.get_capabilities(&gpu.adapter);
        // egui wants a non-sRGB target; the zone shader encodes gamma itself.
        let wanted = [wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgba8Unorm];
        config.format = wanted.into_iter().find(|f| caps.formats.contains(f)).unwrap_or(caps.formats[0]);
        log::info!("surface format {:?} (offered {:?})", config.format, caps.formats);
        config.present_mode = present_mode(&caps, self.frame_limit, &window);
        if caps.usages.contains(wgpu::TextureUsages::COPY_SRC) {
            config.usage |= wgpu::TextureUsages::COPY_SRC;
        }
        surface.configure(&gpu.device, &config);
        let depth = depth_view(&gpu.device, config.width, config.height);
        let egui_ctx = egui::Context::default();
        let egui_state = egui_winit::State::new(egui_ctx.clone(), egui::ViewportId::ROOT, &window, Some(window.scale_factor() as f32), None, None);
        let egui_renderer = egui_wgpu::Renderer::new(&gpu.device, config.format, Default::default());
        // Zone, posing and mip pipelines are compiled now, off this thread.
        gpu.warm(&[config.format]);
        window.request_redraw();
        self.next_frame = Some(Instant::now());
        self.gfx = Some(Gfx { window, gpu, surface, config, depth, egui_ctx, egui_state, egui_renderer });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(gfx) = &mut self.gfx else { return };
        let consumed = gfx.egui_state.on_window_event(&gfx.window, &event).consumed;
        match event {
            // Retail takes Close off its window menu (RemoveMenu SC_CLOSE). Ours logs out first in the world
            // ("/shutdown on", exit when the server ends the session); at once elsewhere, with the link quiet for
            // 10 s, or on a second close when no logout could start (engaged, K.O.).
            WindowEvent::CloseRequested => match &mut self.phase {
                Phase::World(w) if w.net.stats.now_ms().saturating_sub(w.net.stats.last_rx_ms.load(std::sync::atomic::Ordering::Relaxed)) < 10_000 => {
                    if w.logging_out && w.shutdown {
                    } else if self.close_asked {
                        event_loop.exit();
                    } else {
                        self.close_asked = true;
                        textcmd::run(w, "/shutdown on", textcmd::Mode::Internal);
                    }
                }
                _ => event_loop.exit(),
            },
            WindowEvent::Resized(size) => {
                log::info!("window resized to {}x{}", size.width, size.height);
                resize_surface(gfx, size);
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                if self.ui_drawn
                    && self.cfg.tool("0021") != 0
                    && let Some(gfx) = &self.gfx
                {
                    self.mouse.apply_hardware(&gfx.window, event_loop, self.index.as_ref());
                }
                if (self.drive.quit && self.drive_done() && !matches!(self.phase, Phase::World(_))) || self.exit_requested {
                    event_loop.exit();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    // Other key functions hide the cursor (FUN_10125f70(0)), except during a drag.
                    let modifier = matches!(code, KeyCode::ShiftLeft | KeyCode::ShiftRight | KeyCode::ControlLeft | KeyCode::ControlRight | KeyCode::AltLeft | KeyCode::AltRight | KeyCode::SuperLeft | KeyCode::SuperRight);
                    if event.state == ElementState::Pressed && !modifier && !self.dragging && !self.left_down {
                        self.mouse.visible = false;
                    }
                    if code == KeyCode::F12 && event.state == ElementState::Pressed && !event.repeat {
                        self.debug_overlay = !self.debug_overlay;
                    }
                    if event.state == ElementState::Pressed && matches!(code, KeyCode::Slash | KeyCode::NumpadDivide) {
                        self.numpad_slash = code == KeyCode::NumpadDivide;
                    }
                    if !event.repeat {
                        self.input.key(code, event.state == ElementState::Pressed, &event.logical_key);
                    }
                }
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::CursorEntered { .. } => {
                self.cursor_in = true;
                self.mouse.invalidate();
            }
            WindowEvent::CursorLeft { .. } => self.cursor_in = false,
            // Config-tool 0035 = 0: no sound while the window is not focused (no DSBCAPS_GLOBALFOCUS).
            WindowEvent::Focused(f) => {
                if !f {
                    self.input.clear();
                }
                let (m, e) = self.volume_factors();
                if self.cfg.tool("0035") == 0
                    && let Some(a) = self.audio.as_mut()
                {
                    if f {
                        a.set_volumes(m, e);
                    } else {
                        a.set_volumes(0.0, 0.0);
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } if !consumed || state == ElementState::Released => {
                self.mouse_button(button, state == ElementState::Pressed, consumed);
            }
            WindowEvent::CursorMoved { position, .. } => {
                // FUN_100016c0: window-message motion shows the cursor only past 2 px.
                if self.last_cursor.is_none_or(|(x, y)| (position.x - x).abs() > 2.0 || (position.y - y).abs() > 2.0) {
                    self.mouse.visible = true;
                }
                // A left-drag that starts on the map overlay pans it instead of turning the camera.
                let k = self.ui_k;
                let on_overlay = |w: &World, p: (f64, f64)| w.overlay.as_ref().is_some_and(|o| o.has_map() && inside(&o.rect, (p.0 as f32 * k.0, p.1 as f32 * k.1)));
                if let (true, Some(a), Some((x, y)), Phase::World(world)) = (self.left_down, self.press_at, self.last_cursor, &mut self.phase)
                    && on_overlay(world, a)
                {
                    if let Some(o) = world.overlay.as_mut() {
                        o.pan((position.x - x) as f32 * k.0, (position.y - y) as f32 * k.1);
                    }
                    self.last_cursor = Some((position.x, position.y));
                    return;
                }
                let looking = self.dragging || self.middle_down || (self.left_down && self.press_at.is_some_and(|a| (a.0 - position.x).abs() + (a.1 - position.y).abs() >= 5.0));
                if let (true, Some((x, y))) = (looking, self.last_cursor) {
                    match &mut self.phase {
                        // The map's input context (0x26) has no camera functions; its drag scrolls the map.
                        Phase::World(world) if world.map.is_some() || world.cnq.is_some() => {}
                        // Mouse/Cam. 0x8b / 0x8c invert the third-person camera's X / Y (FUN_1001e030).
                        Phase::World(world) if world.locked => {
                            let (ix, iy) = (if world.game.get(0x8b) == 1 { -1.0 } else { 1.0 }, if world.game.get(0x8c) == 1 { -1.0 } else { 1.0 });
                            world.cam_turn.0 += ((position.x - x) * 0.008 * ix) as f32;
                            world.cam_turn.1 -= ((position.y - y) * 0.02 * iy) as f32;
                        }
                        Phase::World(world) if world.unlock_ticks > 0.0 || world.camst.first => {}
                        Phase::World(world) => {
                            let (ix, iy) = (if world.game.get(0x8b) == 1 { -1.0 } else { 1.0 }, if world.game.get(0x8c) == 1 { -1.0 } else { 1.0 });
                            camera::drag(world, ((position.x - x) * 0.008 * ix) as f32, ((position.y - y) * 0.03 * iy) as f32);
                        }
                        _ => {}
                    }
                }
                self.last_cursor = Some((position.x, position.y));
            }
            WindowEvent::MouseWheel { delta, .. } if !consumed => {
                if let Phase::World(world) = &mut self.phase {
                    let d = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                    };
                    let k = self.ui_k;
                    let at = self.last_cursor.map(|c| (c.0 as f32 * k.0, c.1 as f32 * k.1));
                    if let (Some(o), Some(p)) = (world.overlay.as_mut(), at)
                        && o.has_map()
                        && inside(&o.rect, p)
                    {
                        o.zoom_at(p, d);
                        return;
                    }
                    // A focused list takes the wheel (FUN_10125360: 0x16 / 0x17 to the focus window).
                    if self.game_ui.as_ref().is_some_and(|g| g.wants_wheel()) {
                        self.list_wheel += d;
                        return;
                    }
                    // Field of view, as the keys (and retail's wheel on keyboard type 2, FUN_10253af0).
                    if !world.locked {
                        self.wheel += d;
                    }
                }
            }
            _ => {}
        }
    }
}

/// The monitor a full-screen window goes on: the configured one by name, else the current one (Wayland often
/// can't say: None), else the first matching `size`, else the first.
fn pick_monitor(preferred: &str, current: Option<winit::monitor::MonitorHandle>, all: impl Iterator<Item = winit::monitor::MonitorHandle>, size: [u32; 2]) -> Option<winit::monitor::MonitorHandle> {
    let all: Vec<_> = all.collect();
    let named = all.iter().find(|m| !preferred.is_empty() && m.name().as_deref() == Some(preferred)).cloned();
    named.or(current).or_else(|| all.iter().find(|m| m.size().width == size[0] && m.size().height == size[1]).cloned()).or_else(|| all.into_iter().next())
}

/// The monitors for the settings screen: (name, "name  W x H").
fn monitor_list(all: impl Iterator<Item = winit::monitor::MonitorHandle>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    // winit can list an output more than once.
    for m in all {
        let Some(name) = m.name() else { continue };
        if !out.iter().any(|o| o.0 == name) {
            let s = m.size();
            out.push((name.clone(), format!("{name}  {} x {}", s.width, s.height)));
        }
    }
    out
}

/// The swapchain and depth buffer at a new window size.
fn resize_surface(gfx: &mut Gfx, size: winit::dpi::PhysicalSize<u32>) {
    gfx.config.width = size.width.max(1);
    gfx.config.height = size.height.max(1);
    gfx.surface.configure(&gfx.gpu.device, &gfx.config);
    gfx.depth = depth_view(&gfx.gpu.device, gfx.config.width, gfx.config.height);
}

/// Window mode 0034 (FUN_10015830): 0 full screen at the video mode nearest `size` (borderless where there is
/// none, as on Wayland), 3 borderless at the desktop size; 1 windowed and 2 borderless at `size` are windows.
fn fullscreen(mode: u32, size: [u32; 2], monitor: Option<winit::monitor::MonitorHandle>) -> Option<winit::window::Fullscreen> {
    use winit::window::Fullscreen;
    match mode {
        3 => Some(Fullscreen::Borderless(monitor)),
        // Borderless at (or over) the monitor's size covers all of it, panels included, as games do: that is a
        // fullscreen surface on Wayland, since a plain window is fitted to the work area.
        2 if monitor.as_ref().is_some_and(|m| size[0] >= m.size().width && size[1] >= m.size().height) => Some(Fullscreen::Borderless(monitor)),
        0 => {
            let best = monitor.as_ref().and_then(|m| {
                m.video_modes().min_by_key(|v| {
                    let s = v.size();
                    ((s.width as i64 - size[0] as i64).abs() + (s.height as i64 - size[1] as i64).abs(), u32::MAX - v.refresh_rate_millihertz())
                })
            });
            Some(best.map_or(Fullscreen::Borderless(monitor), Fullscreen::Exclusive))
        }
        _ => None,
    }
}

/// Vsync when the limit is at or under the monitor's refresh rate; above it or unlimited, Mailbox, then
/// Immediate, so FIFO does not hold the rate to the monitor.
fn present_mode(caps: &wgpu::SurfaceCapabilities, limit: u32, window: &Window) -> wgpu::PresentMode {
    let refresh = window.current_monitor().and_then(|m| m.refresh_rate_millihertz()).map_or(60.0, |mhz| mhz as f32 / 1000.0);
    if limit != 0 && limit as f32 <= refresh + 0.5 {
        return wgpu::PresentMode::AutoVsync;
    }
    [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate].into_iter().find(|m| caps.present_modes.contains(m)).unwrap_or(wgpu::PresentMode::AutoVsync)
}

fn save_capture(gpu: &Gpu, buf: &wgpu::Buffer, size: [u32; 2], row: u32, format: wgpu::TextureFormat, path: &std::path::Path) -> Result<()> {
    buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None })?;
    let data = buf.slice(..).get_mapped_range().map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let bgra = matches!(format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
    let mut pixels = Vec::with_capacity((size[0] * size[1] * 4) as usize);
    for y in 0..size[1] {
        for px in data[(y * row) as usize..(y * row + size[0] * 4) as usize].chunks_exact(4) {
            pixels.extend_from_slice(&if bgra { [px[2], px[1], px[0], 255] } else { [px[0], px[1], px[2], 255] });
        }
    }
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path)?), size[0], size[1]);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header()?.write_image_data(&pixels)?;
    Ok(())
}

pub fn run(cfg: Config, drive: Drive) -> Result<()> {
    let event_loop = EventLoop::new()?;
    let mut app = App::new(cfg, drive);
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// camera_collision_spec.md 2.2: 0 within 0.2, linear to 0.5, nothing with the eye more than 0.5 above the point
    /// (y down), no lower limit below it.
    #[test]
    fn near_camera_fade_cylinder() {
        let p = Vec3::new(0.0, -1.5, 0.0);
        let at = |x: f32, y: f32| near_camera_fade(p, Vec3::new(x, y, 0.0));
        assert_eq!(at(0.1, -1.5), 0.0);
        assert!((at(0.35, -1.5) - 0.5).abs() < 1e-5);
        assert_eq!(at(0.6, -1.5), 1.0);
        assert_eq!(at(0.1, -2.1), 1.0);
        assert_eq!(at(0.1, -1.9), 0.0);
        assert_eq!(at(0.1, 5.0), 0.0);
    }

    /// s2c 0x017 lines and modes (LAB_100e37a0): GM tag, yell zone, system header, dropped kinds.
    #[test]
    fn chat_kinds() {
        assert_eq!(chat_line(0, 1, 0, "Rabbit", "hi", ""), Some(("[GM]Rabbit : hi".into(), 9)));
        assert_eq!(chat_line(3, 0, 0, "Rabbit", "hi", ""), Some(("Rabbit>> hi".into(), 0xc)));
        assert_eq!(chat_line(0x1a, 0, 230, "Rabbit", "hi", "Southern San d'Oria"), Some(("Rabbit[Southern San d'Oria]: hi".into(), 0xb)));
        assert_eq!(chat_line(6, 0, 0, "", "Restarting", ""), Some(("----== SystemMessage ==----\nRestarting".into(), 0xc8)));
        assert_eq!(chat_line(0x22, 0, 0, "Rabbit", "hi", ""), Some(("Rabbit(J) : hi".into(), 0xdc)));
        assert_eq!(chat_line(0x11, 0, 0, "Rabbit", "text only", ""), Some(("text only".into(), 0x79)));
        assert_eq!(chat_line(0xc, 0, 0, "GM", "menu", ""), None);
        assert_eq!(chat_line(2, 0, 0, "Rabbit", "hi", ""), None);
    }

    /// A steady slope is drawn exactly (one step late); the downhill bob (drops 0.046, 0.083, 0.046 ...) is not.
    #[test]
    fn drawn_steps_smooth_the_bob() {
        let ramp: Vec<Vec3> = (0..6).map(|k| Vec3::new(k as f32 * 0.17, k as f32 * 0.06, 0.0)).collect();
        let h = [ramp[2], ramp[3], ramp[4], ramp[5]];
        assert!((drawn_between_steps(&h, 0.5) - ramp[3].lerp(ramp[4], 0.5)).length() < 1e-5);
        let drops = [0.046f32, 0.083, 0.046, 0.046, 0.083, 0.046, 0.046, 0.083];
        let mut y = vec![0f32];
        for d in drops {
            y.push(y.last().unwrap() + d);
        }
        let p: Vec<Vec3> = y.iter().map(|&y| Vec3::new(0.0, y, 0.0)).collect();
        let drawn: Vec<f32> = (0..5).map(|k| drawn_between_steps(&[p[k], p[k + 1], p[k + 2], p[k + 3]], 0.0).y).collect();
        for w in drawn.windows(2) {
            assert!((w[1] - w[0] - 0.175 / 3.0).abs() < 1e-4, "{drawn:?}");
        }
    }

    #[test]
    fn channel_swap() {
        assert_eq!(strip_channel("/party hello there"), "hello there");
        assert_eq!(strip_channel("/t Someone hi"), "hi");
        assert_eq!(strip_channel("/l2 x"), "x");
        assert_eq!(strip_channel("hello"), "hello");
        assert_eq!(strip_channel("/wave"), "/wave");
    }

    #[test]
    fn plate_depth_scale() {
        assert_eq!(plate_scale(0.5), None);
        assert_eq!(plate_scale(5.0), Some(1.0));
        for (d, s) in [(6.0, 0.9125), (10.0, 0.5625), (20.0, 0.3125), (50.0, 0.1625)] {
            assert!((plate_scale(d).unwrap() - s).abs() < 1e-4, "{d}");
        }
    }
}
