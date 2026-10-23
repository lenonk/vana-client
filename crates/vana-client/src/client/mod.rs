//! The windowed client: login and character select (in place of PlayOnline), then the world.

pub mod ground;
pub mod combat;
mod actmotion;
mod flyover;
mod creator;
pub use creator::{ScreenShot as CreatorScreenShot, screen_shot as creator_screen_shot};
pub use hudtop::topbar_shot;
mod event;
mod game_ui;
mod hudtop;
mod map;
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

use crate::config::Config;
use crate::dat::character::{Character as Model, Look};
use crate::dat::{DatIndex, zone};
use crate::net::auth::{self, Command, Reply, Request};
use crate::net::lobby::{Character, Lobby, MapLogin, NewCharacter};
use crate::render::{Camera, Gpu, ZoneRenderer, depth_view};
use anyhow::Result;
use glam::{Vec3, Vec4Swizzles};
use game_ui::{GOLD, GREY, GameUiData, GameUi, WHITE, YELLOW};
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
            Task::CharacterLoaded(..) => "model loaded",
            Task::GameUi(_) => "game ui",
            Task::Flyover(_) => "flyover",
            Task::Nation(..) => "nation",
            Task::Select(..) => "select",
            Task::CreatorBackdrop(_) => "backdrop",
            Task::Dancer(_) => "dancer",
            Task::Battle(_) => "battle text",
            Task::Names(_) => "names",
        }
    }
}

enum Task {
    LoggedIn(Result<(Lobby, Vec<Character>)>),
    AccountCreated(Result<()>),
    Lobby(Result<(Lobby, Vec<Character>)>),
    Selected(Result<MapLogin>),
    ZoneLoaded(u16, Result<(zone::ZoneScene, ground::Ground, Option<crate::dat::weather::ZoneWeather>, Arc<crate::render::ZoneGeometry>, ZoneEffects)>),
    AreaLoaded(u16, Result<(zone::ZoneScene, ground::Ground, Option<crate::dat::weather::ZoneWeather>, Arc<crate::render::ZoneGeometry>, Option<crate::fx::lib::Library>)>),
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
    /// Actor +0x130 bit 7 (0x00E u8 @0x2B bit 0x08).
    untargetable: bool,
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
/// and the scene to black over 60 ticks, 8 frames pass (FUN_10233d90), the next screen starts, and 'sfin' + 'ffin'
/// bring both back over 60 ticks. The creator brings in only its 2D layer (FUN_10233db0); its scene follows once
/// the dancer shows (FUN_10245fb0).
struct LobbyFade {
    ui: f32,
    scene: f32,
    ui_to: f32,
    scene_to: f32,
    hold: u32,
    next: Option<Box<dyn FnOnce(&mut App)>>,
}

impl Default for LobbyFade {
    fn default() -> Self {
        LobbyFade { ui: 0.0, scene: 0.0, ui_to: 1.0, scene_to: 1.0, hold: 0, next: None }
    }
}

impl LobbyFade {
    /// Fades out, then runs `next` (a change asked for meanwhile is dropped).
    fn out(&mut self, next: Box<dyn FnOnce(&mut App)>) {
        if self.next.is_some() {
            return;
        }
        (self.ui_to, self.scene_to, self.hold, self.next) = (0.0, 0.0, 0, Some(next));
    }

    /// The next screen fades in; the creator's scene waits for its dancer.
    fn enter(&mut self, scene: bool) {
        (self.ui_to, self.scene_to) = (1.0, if scene { 1.0 } else { 0.0 });
    }

    /// One frame of `ticks`; returns the change to make once faded out and held.
    fn step(&mut self, ticks: f32) -> Option<Box<dyn FnOnce(&mut App)>> {
        let to = |v: f32, t: f32| if v < t { (v + ticks / 60.0).min(t) } else { (v - ticks / 60.0).max(t) };
        self.ui = to(self.ui, self.ui_to);
        self.scene = to(self.scene, self.scene_to);
        if self.next.is_some() && self.ui <= 0.0 && self.scene <= 0.0 {
            self.hold += 1;
            if self.hold > 8 {
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
fn login_window(lu: &mut GameUi, input: &UiInput, size: [u32; 2], fields: [&mut String; 4], focus: &mut usize, busy: bool, status: &str, version: &str) -> Option<LoginChoice> {
    const CHOICES: [LoginChoice; 3] = [LoginChoice::LogIn, LoginChoice::Create, LoginChoice::Exit];
    const ITEMS: usize = 7;
    let (sw, sh) = (size[0] as f32, size[1] as f32);
    let (ww, wh) = (380.0, 140.0);
    let (wx, wy) = (((sw - ww) / 2.0).round(), (sh * 440.0 / 768.0).round());
    let row = |i: usize| wy + 14.0 + i as f32 * 22.0;
    let field_rect = |i: usize| [wx + 148.0, row(i), ww - 164.0, 18.0];
    let ay = row(4) + 6.0;
    let actions = [("Log in", wx + 40.0), ("Create account", wx + 150.0), ("Exit", wx + 310.0)];
    let arects = [0, 1, 2].map(|k| [actions[k].1 - 4.0, ay - 2.0, lu.text_width(actions[k].0) + 8.0, 20.0]);

    let mut chosen = None;
    // Hovering an action moves the selection there, as in the game's menus.
    if input.moved
        && let Some(k) = input.pointer.and_then(|p| (0..3).find(|&k| inside(&arects[k], p)))
    {
        *focus = 4 + k;
    }
    if input.clicked
        && let Some(p) = input.pointer
    {
        if let Some(i) = (0..4).find(|&i| inside(&field_rect(i), p)) {
            *focus = i;
        }
        if let Some(k) = (0..3).find(|&k| inside(&arects[k], p)) {
            *focus = 4 + k;
            chosen = Some(CHOICES[k]);
        }
    }
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
                egui::Key::Escape => *focus = 6,
                egui::Key::Enter => chosen = Some(if *focus < 4 { LoginChoice::LogIn } else { CHOICES[*focus - 4] }),
                _ => {}
            },
            _ => {}
        }
    }

    let help = if busy {
        "Connecting to the server..."
    } else if status.is_empty() {
        "Enter your account information to log in."
    } else {
        status
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
    // The selected action as a menu's selected item: gold (0x80805020) one pixel up-left, with the hand.
    for (k, (label, x)) in actions.iter().enumerate() {
        let on = *focus == 4 + k && !busy;
        let d = if on { -1.0 } else { 0.0 };
        lu.text(label, *x + d, ay + d, 0, if busy { GREY } else if on { GOLD } else { WHITE });
        if on {
            lu.cursor(*x - 2.0 + d, ay + d);
        }
    }
    if busy { None } else { chosen }
}

/// Sends a chat line. /logout and /shutdown send c2s 0x0E7 as FUN_1007b8a0 / FUN_1007bb10 do; the countdown
/// text comes from the server (0x053 #7 / #35).
fn send_chat(world: &mut World, text: String) {
    let cmd = text.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    let arg = text.split_whitespace().nth(1).map(str::to_ascii_lowercase);
    if cmd == "/logout" || cmd == "/shutdown" {
        // Mode: none 0 toggle, "on" 1 (3 for shutdown), "off" 2; Kind 1 logout, 3 shutdown. Status 0, 7 or 0x21 only.
        let shutdown = cmd == "/shutdown";
        let mode = match arg.as_deref() {
            Some("on") if shutdown => 3,
            Some("on") => 1,
            Some("off") => 2,
            _ => 0,
        };
        if matches!(world.my_motion.status, 0 | 7 | 0x21) {
            world.logging_out = match mode {
                0 => !world.logging_out,
                2 => false,
                _ => true,
            };
            world.shutdown = shutdown && world.logging_out;
            let _ = world.net.commands.send(NetCommand::Logout { mode, kind: if shutdown { 3 } else { 1 } });
        }
    } else if cmd == "/attack" || cmd == "/a" {
        // FUN_10079490: disengage when engaged, else attack the target (0x01A action 4 / 2); no target, nothing.
        let c = if world.my_motion.status == 1 { combat::cmd::DISENGAGE } else { combat::cmd::ATTACK };
        if !combat::choose(world, c) {
            log::info!("/attack: no target");
        }
    } else if cmd == "/heal" {
        // FUN_1007adb0: only standing (0), status 7 or healing (0x21); "on" 1, "off" 2, else 0 = toggle.
        let mode = match text.split_whitespace().nth(1).map(str::to_ascii_lowercase).as_deref() {
            Some("on") => 1,
            Some("off") => 2,
            _ => 0,
        };
        if matches!(world.my_motion.status, 0 | 7 | 0x21) {
            let _ = world.net.commands.send(NetCommand::Heal(mode));
        }
    } else if cmd == "/targetcharaeffect" {
        // Command 0x82 (FUN_1007d800): config 0x99 bit 7, which gates only the forced flash; group 9 0x137 / 0x136.
        let off = world.game.get(0x99) & 0x80 == 0;
        let v = world.game.get(0x99);
        world.game.set(0x99, if off { v | 0x80 } else { v & !0x80 });
        world.game_save = true;
        if let Some(bt) = world.battle.clone() {
            let line = bt.group9(if off { 0x137 } else { 0x136 }).unwrap_or_default();
            world.chat.push(line);
        }
    } else if cmd == "/lockon" {
        // Command 0x59 (FUN_100793f0): the lock-on toggle.
        let on = !world.locked;
        target::lock(world, on, "/lockon");
    } else if cmd == "/tell" || cmd == "/t" {
        let mut rest = text.splitn(3, ' ').skip(1);
        match (rest.next().filter(|n| !n.is_empty()), rest.next().map(str::trim).filter(|m| !m.is_empty())) {
            (Some(name), Some(msg)) => {
                let line = stamped(world, format!(">>{name} : {msg}"));
                let c = cfgapply::chat_colour(&world.game, 3);
                world.chat.push_colour(line, c);
                let _ = world.net.commands.send(NetCommand::Tell { name: name.to_string(), text: msg.to_string() });
            }
            // FUN_100775f0 returns an error: ">> line" and group 9 0x78 (FUN_1007f6d0).
            _ => combat::command_error(world, &text),
        }
    } else {
        // The server does not echo our own lines; the client prints them.
        let (kind, msg) = match chat_channel(&cmd) {
            Some(k) => (k, text.split_once(' ').map_or("", |(_, m)| m.trim()).to_string()),
            // Retail prints nothing for a command it does not know; ours may be one it knows.
            None if cmd.starts_with('/') => {
                log::info!("{cmd} is not supported yet");
                return;
            }
            None => (0, text),
        };
        if !msg.is_empty() {
            let line = stamped(world, chat_line(kind, &world.name, &msg));
            let c = cfgapply::chat_colour(&world.game, kind);
            world.chat.push_colour(line, c);
            let _ = world.net.commands.send(NetCommand::Chat { kind, text: msg });
        }
    }
}

/// A chat line with its timestamp (config 0xb5, "%s[%s]%s %s" at 0x1036344c).
fn stamped(world: &World, line: String) -> String {
    match cfgapply::timestamp(&world.game) {
        Some(t) => format!("{t} {line}"),
        None => line,
    }
}

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
        "/yell" | "/y" => 0x1a,
        "/linkshell2" | "/l2" => 0x1b,
        "/unity" | "/u" => 0x21,
        "/assistj" | "/aj" => 0x22,
        "/assiste" | "/ae" => 0x23,
        _ => return None,
    })
}

/// A chat line as the log shows it, by 0x017 kind (inferred punctuation).
fn chat_line(kind: u8, from: &str, text: &str) -> String {
    match kind {
        _ if from.is_empty() => text.to_string(),
        3 => format!("{from}>> {text}"),
        4 => format!("({from}) {text}"),
        5 | 0x1b => format!("<{from}> {text}"),
        0x21 => format!("{{{from}}} {text}"),
        _ => format!("{from} : {text}"),
    }
}

/// Ctrl/Alt + S/T/P/L/U/X (LAB_101a64c0): open the input on a channel, keeping a message being typed
/// minus its own channel command (FUN_101a6550).
fn chat_prefix(world: &mut World, prefix: &str) {
    let typed = if world.typing { std::mem::take(&mut world.chat_input) } else { String::new() };
    let body = strip_channel(typed.trim_start());
    world.chat_input = format!("{prefix}{body}");
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
    if w.full_log.is_some() || w.typing || event::active(w) {
        return;
    }
    let active = w.log_active || w.menu.is_some() || w.win.is_some() || w.action_menu.is_some();
    if w.log_focus != 0 && !active {
        w.log_focus = 0;
        return;
    }
    w.sounds.push(13);
    w.log_focus = (w.log_focus + 1) % 3;
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
        }
        egui::Key::Enter => w.sounds.push(4),
        k => {
            if fl.key(k, held, logwin::full_lines(sh), w.log.history()) {
                w.sounds.push(1);
            }
        }
    }
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
fn name_colour(e: &Entity, me: u32) -> usize {
    match (e.kind, e.mob, e.claim) {
        (EntityKind::Player, _, _) => 0,
        (_, false, _) => 4,
        (_, true, 0) => 5,
        (_, true, c) if c == me => 6,
        _ => 7,
    }
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

/// Name plates over characters (FUN_100ce4d0 -> FUN_10084b20): fontshp glyphs at the model's top,
/// sized by the screen (W, H) * 0.00234375 times the depth scale.
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
    // Invisible helpers (no model or a stub one) only show a name while targeted; faded actors none.
    let visible = |id: &u32, e: &Entity| {
        let faded = world.actors.get(id).is_some_and(|a| a.vanished());
        !faded && (e.kind == EntityKind::Player || world.actors.get(id).is_some_and(|a| a.shown()) || world.target == Some(*id))
    };
    for (id, e) in world.entities.iter().filter(|(id, e)| !e.name.is_empty() && !e.name.starts_with('_') && !e.no_plate && visible(id, e) && !hidden.contains(id)) {
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
            gu.name_plate(&e.name, x, y, scale, name_colour(e, world.my_id), pulse);
        }
    }
    if !hidden.contains(&world.my_id)
        && let Some((x, y, scale, z)) = plate_point(world, world.my_id).and_then(project)
    {
        gu.depth(Some(z));
        gu.name_plate(&world.name, x, y, scale, 0, 1.0);
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
/// for bone-0 points (5 chocobo -1.3, 85 mount -1.3, 63..83 chairs -0.5). Without points: the mesh top.
fn plate_point(world: &World, id: u32) -> Option<Vec3> {
    let (pos, status) = if id == world.my_id { (drawn_pos(world), world.my_motion.status) } else { world.entities.get(&id).map(|e| (e.pos, e.motion.status))? };
    let pos = event::pose(world, id).map_or(pos, |p| p.pos);
    let a = world.actors.get(&id).filter(|a| a.gpu.is_some());
    let point = a.and_then(|a| a.character.as_ref()?.skeleton.points.get(2).copied().map(|p| (p, a.scale)));
    match point {
        Some(((bone, t), s)) => {
            let lift = match status {
                5 | 85 if bone == 0 => -1.3,
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
    // FUN_1022eca0: keytops3 165 + (n >> 2) % 5 at (W - 234, H - 48 + 8 - min(2n, 8)), y scale min(n / 4, 1), n
    // the draws since the open (taken at 30 a second, inferred); the HUD windows are not drawn meanwhile.
    if let Some(t) = world.downloading {
        gu.clear();
        downloading(gu, (t.elapsed().as_secs_f32() * 30.0) as usize + 1, sw, sh);
        return Vec::new();
    }
    let items = gu.menu_items(sw, sh, world.menu_page);
    let dead = world.dead();
    let mut home = false;
    let mut opened = false;
    let mut shrink_log = false;
    let mut pick: Option<usize> = None;
    let mut answer: Option<bool> = None;
    let mut act: Option<u8> = None;
    for ev in &input.events {
        match ev {
            egui::Event::Text(t) | egui::Event::Paste(t) if world.typing && !opened => {
                world.chat_input.extend(t.chars().filter(|c| !c.is_control()));
            }
            // egui turns Ctrl+C / Ctrl+X into Copy / Cut instead of key events.
            egui::Event::Copy if !world.typing && !event::active(world) => check_target(world),
            egui::Event::Cut => chat_prefix(world, "/linkshell2 "),
            egui::Event::Key { key, pressed: true, modifiers, .. } => match key {
                egui::Key::Enter if world.typing => {
                    world.typing = false;
                    let text = std::mem::take(&mut world.chat_input);
                    if !text.trim().is_empty() {
                        send_chat(world, text);
                    }
                }
                egui::Key::Escape if world.typing => {
                    world.typing = false;
                    world.chat_input.clear();
                }
                egui::Key::Backspace if world.typing => {
                    world.chat_input.pop();
                }
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
                egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::Enter | egui::Key::Escape if world.map.is_some() => {
                    if !world.map.as_mut().is_some_and(|m| m.key(*key)) {
                        world.map = None;
                    }
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
                egui::Key::O if modifiers.ctrl || modifiers.alt => {
                    if world.overlay.take().is_none() {
                        let o = map::Overlay::open(world, gu.map_regions());
                        if !o.has_map() {
                            world.sounds.push(crate::audio::Ui::Error as u32);
                        }
                        world.overlay = Some(o);
                    }
                }
                egui::Key::H if modifiers.ctrl || modifiers.alt => send_chat(world, "/heal".into()),
                egui::Key::C if modifiers.ctrl || modifiers.alt => check_target(world),
                // FUN_1012ec60 / FUN_101e9f10.
                egui::Key::E if modifiers.ctrl || modifiers.alt => items::open_equipment(world),
                egui::Key::I if modifiers.ctrl || modifiers.alt => items::open_items(world),
                // Our settings screen, then the Config windows, take the keys while open.
                _ if world.settings.is_some() => {
                    let mut s = world.settings.take().unwrap();
                    if !s.key(*key) {
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
                _ if world.win.is_some() => {
                    items::key(world, *key);
                }
                // A yes/no prompt takes the keys while it is up.
                _ if world.confirm.is_some() => match key {
                    egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::Tab => {
                        if let Some((_, yes)) = &mut world.confirm {
                            *yes = !*yes;
                        }
                    }
                    egui::Key::Enter => answer = world.confirm.map(|c| c.1),
                    egui::Key::Escape => answer = Some(false),
                    _ => {}
                },
                // The action menu (Enter) takes the cursor keys while it is up.
                _ if world.action_menu.is_some() => {
                    let m = world.action_menu.as_mut().unwrap();
                    let n = m.items.len().max(1);
                    match key {
                        egui::Key::ArrowDown => m.sel = (m.sel + 1) % n,
                        egui::Key::ArrowUp => m.sel = (m.sel + n - 1) % n,
                        egui::Key::Enter => act = m.items.get(m.sel).copied(),
                        egui::Key::Escape => world.action_menu = None,
                        _ => {}
                    }
                }
                egui::Key::Minus => {
                    world.menu = if world.menu.is_some() { None } else { Some(0) };
                    world.menu_page = 0;
                }
                egui::Key::Escape if world.menu.is_some() => world.menu = None,
                egui::Key::Escape if world.overlay.is_some() => world.overlay = None,
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
                egui::Key::Enter if dead && !world.homing => home = true,
                // Enter on an NPC target talks to it (FUN_101524e0); otherwise it opens the action menu
                // (FUN_10127790). Inferred: monsters (0x00E @0x25 bit 0x08) take the menu path.
                egui::Key::Enter if !dead => {
                    let npc = world.target.filter(|t| t >> 24 != 0 && world.entities.get(t).is_some_and(|e| !e.mob));
                    match npc {
                        Some(t) => {
                            event::talk(world, t);
                        }
                        None => {
                            world.action_menu = Some(combat::open_menu(world));
                            world.menu = None;
                        }
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
                egui::Key::Tab if !modifiers.any() => App::cycle_target(world),
                _ => {}
            },
            _ => {}
        }
    }

    // Mouse on the main menu: hover moves the cursor, a click picks; the arrows swap pages.
    if let (Some(sel), Some(p), None) = (world.menu, input.pointer, world.confirm) {
        if let Some(k) = items.iter().position(|(_, r)| inside(r, p)) {
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
    if let Some(id) = pick.and_then(|k| items.get(k)).map(|(i, _)| i + 1) {
        match (world.menu_page, id) {
            (0, 2) => items::open_equipment(world),
            (0, 4) => items::open_items(world),
            // Item 12 Map (FUN_1013b3f0 case 0xc; the Mog House branch to myroom is not done).
            (0, 12) => match map::MapWin::open(world, gu.map_regions(), sw, sh) {
                Some(m) => {
                    world.map = Some(m);
                    world.menu = None;
                }
                None => log::info!("map: no map record for zone {}", world.pos_zone),
            },
            // Child 7 Config (FUN_10147fc0 case 7): configwi over the menu, SE 0002.
            (1, 7) => {
                world.config = Some(configui::ConfigUi::open("configwi"));
                world.menu = None;
                world.sounds.push(crate::audio::Ui::Select as u32);
            }
            (1, 11) => world.confirm = Some((Confirm::Shutdown, true)),
            (1, 12) => world.confirm = Some((Confirm::Logout, true)),
            _ => log::info!("main menu item {id} on page {} is not implemented yet", world.menu_page),
        }
        if world.confirm.is_some() || world.win.is_some() {
            world.menu = None;
        }
    }
    // Mouse on the settings screen, else the Config windows.
    if let (Some(s), Some(p)) = (world.settings.as_mut(), input.pointer) {
        s.pointer(p, input.moved, input.clicked, sw, sh);
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
    // Mouse in an event: a click on a choice row picks it, anywhere else advances a prompt.
    if input.clicked && world.event.is_some() {
        let row = event::choice(world).and_then(|_| {
            let r = gu.window_rect("menu    query", sw, sh)?;
            let p = input.pointer?;
            gu.children("menu    query").iter().position(|c| inside(&[r[0] + c[0], r[1] + c[1], c[2].max(132.0), c[3].max(16.0)], p))
        });
        event::click(world, row);
    }

    // Action menu rows: hover moves the cursor, a click on the highlighted row picks it.
    let action_rows = world.action_menu.as_ref().and_then(|m| gu.window_rect(&combat::window_name(m.items.len()), sw, sh).map(|r| (r, combat::rows(r[0], r[1], m.items.len()))));
    if let (Some((_, rows)), Some(p), Some(m)) = (&action_rows, input.pointer, world.action_menu.as_mut())
        && let Some(k) = rows.iter().position(|r| inside(r, p))
    {
        if input.moved || input.clicked {
            m.sel = k;
        }
        if input.clicked {
            act = m.items.get(k).copied();
        }
    }
    if let Some(c) = act {
        world.action_menu = None;
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
    let target = world.target.and_then(|t| world.entities.get(&t));
    let st = crate::render::screens::HudState {
        party: world.vitals.map(|v| vec![(v.3 as f32 / 100.0, v.4 as f32 / 100.0)]).unwrap_or_default(),
        target: target.filter(|_| event::keeps_target(world)).map(|t| (!t.no_gauge).then_some(t.hpp as f32 / 100.0)),
        menu_selected: world.menu.and_then(|k| items.get(k)).map(|(i, _)| *i),
        menu_page: world.menu_page,
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
    let mut map = world.map.take();
    if let Some(m) = map.as_mut() {
        let facing = world.net.me.lock().map_or(0.0, |m| event::rad_of(m.dir));
        m.draw(gu, (world.pos.x, world.pos.z), facing, sw, sh);
    }
    world.map = map;
    let slots = gu.hud(sw, sh, &st);

    // Target cursor (FUN_10149900): anc_l at the target's point, 6 frames of 4 ticks, tint 0xe0808080;
    // locked, windowps 212 over the target window (0x10149bc0).
    if let (Some(t), true) = (world.target, st.target.is_some()) {
        if let Some((x, y)) = target_point(world, t, sw, sh, view_proj) {
            let frame = (world.target_since.elapsed().as_secs_f32() * 60.0 / 4.0) as usize % gu.lib_len("anc     anc_l").max(1);
            gu.sprite("anc     anc_l", frame, x.round(), y.round(), 0xe080_8080);
        }
        if world.locked
            && let Some(r) = gu.window_rect("menu    targetwi", sw, sh)
        {
            gu.sprite("menu    windowps", 212, r[0], r[1], 0x8080_8080);
        }
    }
    // FUN_1015ba60: the log stays open while typing with more than 2 lines, while a window with flag 0x1000 is up
    // (the choice menu *(inf)*), and while a message waits with no more rows showing than its page: older lines
    // decay away and the message holds.
    let page_rows = event::message_ref(world).and_then(|m| m.pages.as_ref().and_then(|(_, p)| p.get(m.page)).map(Vec::len)).unwrap_or(0);
    let keep = (world.typing && world.log.lines() > 2) || event::choice(world).is_some() || (event::prompt(world) && world.log.lines() <= page_rows.max(1));
    // Another window taking the keys takes the focus; an event closes the full log (FUN_10133000).
    if world.menu.is_some() || world.win.is_some() || world.action_menu.is_some() || world.confirm.is_some() || world.config.is_some() || event::active(world) {
        world.log_active = false;
    }
    if event::active(world) && world.full_log.take().is_some() {
        world.log_focus = 0;
    }
    let tw = world.log.update(gu, &world.chat, sw, keep, shrink_log, world.log_active);
    let log = world.log.frame(sw, sh);
    let event_window = event_window(gu, world, sw, sh, tw);
    let bars = bar_text(gu, world, &items);
    let cam = App::camera(world);
    let mut top = std::mem::take(&mut world.hud_top);
    // The compass follows region 0's top (UI object +0xa0, refreshed by FUN_10159080), i.e. the log's current top.
    top.draw(gu, world, sw, sh, &bars, cam.target - cam.eye, log.rect[1]);
    world.hud_top = top;
    let party = crate::render::screens::party_menu(st.party.len());
    let mut blockers: Vec<[f32; 4]> = [party.as_str()].iter().filter_map(|n| gu.window_rect(n, sw, sh)).collect();
    blockers.extend(world.overlay.as_ref().filter(|o| o.has_map()).map(|o| o.rect));
    // Event text is in the log, drawn as usual; only placed narration (text codes 02 / 03) has its own window.
    if let Some(r) = event_window {
        blockers.push(r);
    } else if let Some(fl) = world.full_log {
        // The small logs are closed while the full log is open (FUN_1015c280).
        blockers.push(world.log.draw_full(gu, &fl, sw, sh, world.typing, world.game.get(0xb4) != 0));
    } else if log.rect[3] > 0.0 {
        let [x, y, w, h] = log.rect;
        gu.frame(x, y, w, h);
        let newest = world.log.draw(gu, &log, world.typing);
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
            gu.sprite("menu    windowps", logwin::caption(""), x + tw - 352.0, y, 0x8080_8080);
        }
        blockers.push(log.rect);
    }
    // msgline (16, H - 38), (textw + 14) x 22, windowps 37, text at +2, +2 (FUN_101f62a0).
    if world.typing {
        let (x, y, w) = (16.0, sh - 38.0, tw + 14.0);
        gu.frame(x, y, w, 22.0);
        gu.sprite_from("menu    windowps", 37, 1, x, y, 0x8080_8080);
        let shown = fit_tail(gu, &world.chat_input, tw - 20.0);
        gu.text(&shown, x + 2.0 + 2.0, y + 2.0, 0, WHITE);
        if gu.blink() {
            gu.rect(x + 4.0 + gu.text_width(&shown), y + 3.0, 2.0, 14.0, [1.0, 1.0, 1.0, 0.9]);
        }
        blockers.push([x, y, w, 22.0]);
    }
    if st.target.is_some() {
        blockers.extend(gu.window_rect("menu    targetwi", sw, sh));
    }
    blockers.extend(items::draw(gu, world, sw, sh));
    if let Some(c) = &world.config {
        blockers.extend(c.draw(gu, &world.game, sw, sh));
    }
    if let Some(s) = &world.settings {
        blockers.push(s.draw(gu, sw, sh));
    }
    // The map covers the screen (mapframe takes the mouse).
    if world.map.is_some() {
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
        blockers.extend(gu.window_rect("menu    query", sw, sh));
    }
    if world.menu.is_some() {
        blockers.extend(gu.window_rect(crate::render::screens::menu_window(world.menu_page), sw, sh));
    }
    // The action menu (FUN_101276a0): label sprites, the highlighted one tinted 0x80805020.
    if let (Some(m), Some((r, rows))) = (&world.action_menu, &action_rows) {
        gu.window(&combat::window_name(m.items.len()), sw, sh);
        for (k, (c, row)) in m.items.iter().zip(rows).enumerate() {
            let (lib, sprite) = combat::label(*c);
            gu.sprite(lib, sprite, row[0], row[1], if k == m.sel { 0x8080_5020 } else { 0x8080_8080 });
            if k == m.sel {
                gu.cursor(row[0] - 2.0, row[1]);
            }
        }
        blockers.push(*r);
    }

    // The yes/no prompt (FUN_102075f0) with the message from group 0xe.
    if let Some((kind, yes)) = world.confirm {
        let msg = match (kind, world.logging_out) {
            (Confirm::Logout, false) => gu.menu_msg(0x48, "Log out?"),
            (Confirm::Logout, true) => gu.menu_msg(0x49, "Cancel current logout process?"),
            (Confirm::Shutdown, false) => gu.menu_msg(0x277, "Shut down?"),
            (Confirm::Shutdown, true) => gu.menu_msg(0x278, "Cancel shut down process?"),
        };
        let ww = (gu.text_width(&msg) + 48.0).max(200.0);
        let (wx, wy, wh) = (((sw - ww) / 2.0).round(), (sh / 2.0 - 40.0).round(), 56.0);
        gu.frame(wx, wy, ww, wh);
        gu.text(&msg, wx + ww / 2.0, wy + 8.0, 1, WHITE);
        for (k, label) in ["Yes", "No"].iter().enumerate() {
            let x = wx + ww / 2.0 + if k == 0 { -44.0 } else { 20.0 };
            let hit = [x - 4.0, wy + 28.0, gu.text_width(label) + 8.0, 20.0];
            if let Some(p) = input.pointer.filter(|p| inside(&hit, *p)) {
                let _ = p;
                if input.moved || input.clicked {
                    world.confirm = Some((kind, k == 0));
                }
                if input.clicked {
                    answer = Some(k == 0);
                }
            }
            gu.text(label, x, wy + 30.0, 0, WHITE);
            if (k == 0) == world.confirm.map_or(yes, |c| c.1) {
                gu.cursor(x - 2.0, wy + 30.0);
            }
        }
        blockers.push([wx, wy, ww, wh]);
    }
    for t in &slots {
        match t.what {
            "target name" => {
                if let Some(e) = target {
                    let max = gu.window_rect("menu    targetwi", sw, sh).map_or(100.0, |r| r[2] - 13.0);
                    let mut name = e.name.clone();
                    while gu.shape_width(&name) > max && !name.is_empty() {
                        name.pop();
                    }
                    gu.shape_text(&name, t.x, t.y, 0x8080_8080, false);
                }
            }
            "member name" => {
                if let Some((_, _, tp, _, _)) = world.vitals {
                    gu.shape_name(&world.name, t.x, t.y, 0x8080_8080);
                    // TP (FUN_101f9940): right-aligned at (+0x30, +10), blue from 1000; with config 0xae bit 0 (ours
                    // `party_tp`), except trust rows (FUN_101f8ee0: index 0x700 and up). Nothing else moves.
                    if world.party_tp {
                        gu.shape_text(&tp.to_string(), t.x + 48.0, t.y + 10.0, if tp > 999 { 0x8020_60d0 } else { 0x8060_6060 }, true);
                    }
                }
            }
            "HP" => {
                if let Some((hp, _, _, hpp, _)) = world.vitals {
                    let tint = gu.hp_tint(hpp);
                    gu.shape_text(&hp.to_string(), t.x, t.y, tint, true);
                }
            }
            "MP" => {
                if let Some((_, mp, _, _, _)) = world.vitals {
                    gu.shape_text(&mp.to_string(), t.x, t.y, 0x8080_8080, true);
                }
            }
            _ => {}
        }
    }

    // KO: return home, with the time before the server does it anyway.
    if dead {
        let (ww, wh) = (240.0, 66.0);
        let (wx, wy) = (((sw - ww) / 2.0).round(), (sh / 2.0 + 60.0).round());
        gu.frame(wx, wy, ww, wh);
        gu.text("Return to your home point?", wx + ww / 2.0, wy + 8.0, 1, WHITE);
        if let Some(t) = world.home_at {
            let s = t.saturating_duration_since(Instant::now()).as_secs();
            gu.text(&format!("Time remaining: {}:{:02}", s / 60, s % 60), wx + ww / 2.0, wy + 24.0, 1, GREY);
        }
        let label = if world.homing { "Returning..." } else { "Home Point" };
        let (lx, ly) = (wx + ww / 2.0 - gu.text_width(label) / 2.0, wy + 42.0);
        gu.text(label, lx, ly, 0, if world.homing { GREY } else { WHITE });
        let hit = [lx - 4.0, ly - 2.0, gu.text_width(label) + 8.0, 20.0];
        if !world.homing {
            gu.cursor(lx - 2.0, ly);
            if input.clicked && input.pointer.is_some_and(|p| inside(&hit, p)) {
                home = true;
            }
        }
        blockers.push([wx, wy, ww, wh]);
    }
    match (answer, world.confirm) {
        (Some(true), Some((kind, _))) => {
            world.confirm = None;
            // LAB_10159f00 / LAB_10159ee0: Yes runs "/logout" or "/shutdown" again (a toggle, which also cancels).
            send_chat(world, if kind == Confirm::Shutdown { "/shutdown".into() } else { "/logout".into() });
        }
        (Some(false), _) => world.confirm = None,
        _ => {}
    }

    if home && !world.homing {
        world.homing = true;
        let _ = world.net.commands.send(NetCommand::HomePoint);
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
        world.want_settings = true;
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
fn bar_text(gu: &GameUi, world: &World, items: &[(usize, [f32; 4])]) -> hudtop::BarText {
    let mut bars = hudtop::BarText::default();
    let map0 = world.map.as_ref().filter(|m| m.has_menu()).map(|m| ("menu    map0", m.cursor));
    let cfg_focus = world.config.as_ref().and_then(|c| c.focus());
    let focus = cfg_focus.as_ref().map(|(n, c)| (n.as_str(), *c)).or(items::focus(world)).or(map0).or_else(|| world.menu.and_then(|k| items.get(k)).map(|(i, _)| (crate::render::screens::menu_window(world.menu_page), *i as u16 + 1)));
    // mapframe's base draw (FUN_101e6ca0) puts the zone name (FUN_101c2090(zone, 1)) in helpwind slot 2.
    if world.map.is_some()
        && let Some(names) = world.names.as_ref()
    {
        match crate::dat::names::NameSource::place(&**names, world.pos_zone as u32).filter(|n| !n.is_empty()) {
            Some(n) => bars.help[2] = n,
            None => missing_place(world.pos_zone),
        }
    }
    if let Some((name, child)) = focus {
        let [help, title] = gu.focus_text(name, child);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
        if child == configui::SETTINGS {
            bars.help[1] = configui::SETTINGS_HELP.into();
        }
    }
    if let Some(s) = &world.settings {
        (bars.help[1], bars.title[1]) = (s.help(), "Settings".into());
    }
    if let Some(m) = &world.action_menu {
        bars.help[0] = m.items.get(m.sel).and_then(|&c| gu.cmd_help(c)).unwrap_or_default();
        bars.title[0] = gu.title_msg(if world.my_motion.status == 1 { 7 } else { 5 }).unwrap_or_default();
    }
    bars
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
    /// The motion being blended out, its ticks, and blend progress in ticks (16 = done).
    prev: Option<(String, f32)>,
    blend: f32,
    /// Height of the posed model's top above its feet (world units, y down so negative).
    top: f32,
    /// An attack, damage or death motion played once over the status motion.
    oneshot: Option<combat::OneShot>,
    /// The attack scheduler used last (FUN_100d46f0 avoids repeating it).
    last_attack: String,
    /// Motion files on top of the model (event banks, battle files), newest last.
    extra: actmotion::Extra,
    /// A scheduler's motions (event motions, weapon draw / sheathe), and one waiting for its files.
    play: Option<actmotion::Play>,
    pending: Option<actmotion::Pending>,
    /// Battle files on the list (FUN_100cf650 while engaged) and the engaged state they follow.
    battle: actmotion::Battle,
    /// Blend length of the current switch in ticks (scheduler op 0x05 u16 @0x18, else 16).
    blend_len: f32,
    /// Alpha 0..0x80 (model colour high byte) and a fade toward a target (op 0x6c, "kesu").
    alpha: f32,
    /// The target flash state (FUN_100c94a0).
    flash: flash::Flash,
    fade: Option<actmotion::Fade>,
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
    look: actmotion::Look,
    emote: Option<String>,
    /// Rest-pose bones (model space), for attach points of a look target.
    rest: Vec<(glam::Quat, Vec3)>,
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
            blend: 0.0,
            top: -2.0,
            oneshot: None,
            last_attack: String::new(),
            extra: Vec::new(),
            play: None,
            pending: None,
            battle: Default::default(),
            blend_len: crate::motion::BLEND_TICKS,
            alpha: actmotion::OPAQUE,
            flash: Default::default(),
            fade: None,
            bones: Vec::new(),
            scale: 1.0,
            top_key: String::new(),
            find_cache: Default::default(),
            failed: false,
            mouth: None,
            look: Default::default(),
            emote: None,
            rest: Vec::new(),
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
    Look { race: (tbl[0] >> 8) as u8, face: tbl[0] & 0xFF, head: id(1), body: id(2), hands: id(3), legs: id(4), feet: id(5), main: id(6), sub: id(7) }
}

const TAU_4096: f32 = std::f32::consts::TAU / 4096.0;

/// Where and how an actor is drawn this frame: position, facing, status, motion ratio, size.
#[derive(Clone, Copy)]
struct Place {
    pos: Vec3,
    forward: Vec3,
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
    let a = (h[0] + h[1] + h[2]) / 3.0;
    let b = (h[1] + h[2] + h[3]) / 3.0;
    a.lerp(b, t)
}

/// A bound actor draws from its VM body while the event runs (status taken as 0 *(inf)*); otherwise ourselves
/// from the session, others from their entity.
fn actor_place(w: &World, id: u32, my_dir: u8) -> Option<Place> {
    let size = if id == w.my_id { w.my_motion.graph_size } else { w.entities.get(&id)?.motion.graph_size };
    if let Some(p) = event::pose(w, id) {
        return Some(Place { pos: p.pos, forward: Vec3::new(p.yaw.cos(), 0.0, -p.yaw.sin()), status: 0, ratio: p.ratio, size });
    }
    if id == w.my_id {
        let m = w.my_motion;
        return Some(Place { pos: drawn_pos(w), forward: forward_from_dir(my_dir), status: m.status, ratio: if w.moving { w.move_tier } else { 0.0 }, size });
    }
    let e = w.entities.get(&id)?;
    Some(Place { pos: e.pos, forward: Vec3::new(e.yaw.cos(), 0.0, -e.yaw.sin()), status: e.motion.status, ratio: e.ratio, size })
}

/// An actor's attach point `k` in the world from its rest pose (vtable +0x1c4); without one its position lifted
/// by 1.2 x its height (FUN_10086820's fallback).
fn attach_point(w: &World, places: &HashMap<u32, Place>, id: u32, k: u16) -> Option<Vec3> {
    let p = places.get(&id)?;
    let a = w.actors.get(&id)?;
    let ch = a.character.as_ref()?;
    let model = actor_matrix(p.pos, p.forward) * glam::Mat4::from_scale(Vec3::splat(a.scale));
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
    pet: u16,
    /// The area environments' tints this frame, and the floor to find an actor's area on.
    areas: &'a HashMap<u32, Vec3>,
    ground: Option<&'a ground::Ground>,
}

impl crate::fx::Actors for FxActors<'_> {
    fn matrix(&self, id: u32) -> Option<glam::Mat4> {
        let p = self.places.get(&id)?;
        Some(actor_matrix(p.pos, p.forward))
    }

    fn point(&self, id: u32, k: u16) -> Option<Vec3> {
        let a = self.actors.get(&id)?;
        let ch = a.character.as_ref()?;
        let (bone, off) = ch.skeleton.points.get(k as usize)?;
        a.rest.get(*bone as usize).map(|(q, t)| (*t + *q * *off) * a.scale)
    }

    fn scale(&self, id: u32) -> f32 {
        self.actors.get(&id).map_or(1.0, |a| a.scale)
    }

    fn files(&self, id: u32) -> (Vec<u32>, Vec<u32>) {
        let Some(a) = self.actors.get(&id) else { return (Vec::new(), Vec::new()) };
        let Some(ch) = a.character.as_ref() else { return (Vec::new(), Vec::new()) };
        (ch.part_files.clone(), a.extra.iter().rev().map(|(_, f)| f.id).chain(ch.files.iter().copied()).collect())
    }

    fn hidden(&self, id: u32) -> bool {
        self.actors.get(&id).is_none_or(|a| a.vanished())
    }

    fn models(&self) -> Vec<(u32, Vec<u32>)> {
        self.actors.iter().filter(|(id, _)| self.places.contains_key(id)).filter_map(|(id, a)| a.character.as_ref().map(|ch| (*id, ch.files.clone()))).collect()
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
        if id == self.me || (self.pet != 0 && e.is_some_and(|e| e.index == self.pet)) {
            1
        } else if e.is_some_and(|e| e.flags & 1 != 0) {
            2
        } else {
            8
        }
    }
}

/// Loaded looks kept while no actor wears them.
const MODEL_CACHE: usize = 96;

/// Model space faces +x; place it at `pos` facing `forward` (xz).
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
    zone: Option<u16>,
    loading: Option<u16>,
    area: AreaSwitch,
    /// Event op 0xd9: effect sounds get 20 voices while an event runs (FUN_10038330).
    event_voices: bool,
    /// Looks set by event op 0xb6, for the model loader.
    event_looks: Vec<(u32, Source)>,
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
    /// Zone lines with a destination (tag + destination tag).
    zone_lines: Vec<zone::Region>,
    zoning_since: Option<Instant>,
    chat: combat::ChatLog,
    chat_input: String,
    /// The log window's size and scroll state, and the top bar / Network / compass state.
    log: logwin::LogWin,
    hud_top: hudtop::HudTop,
    /// Who sent us tells, most recent first (Ctrl+R).
    tells: Vec<String>,
    /// Battle message table, names and colours (loaded in the background).
    battle: Option<Arc<crate::dat::battle::BattleText>>,
    /// Melee results waiting for the attacker's hit.
    pending: Vec<combat::Pending>,
    /// The action menu (Enter), and when the world started (a seed for random motion picks).
    action_menu: Option<combat::ActionMenu>,
    started: Instant,
    logging_out: bool,
    target: Option<u32>,
    /// Target lock-on (DAT_10565d34 +0x5c bits 0..1 != 0), the locked camera, ticks left in which the free
    /// camera ignores manual turns after a lock (DAT_10444464), auto lock-on (config 7 = 0), when the target
    /// was set, and manual camera turns waiting for the locked camera.
    locked: bool,
    cam: target::LockCam,
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
    /// Config `party_tp` (retail config 0xae bit 0).
    party_tp: bool,
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
    /// While dead: when the server will send us home regardless, and whether we already asked to go.
    home_at: Option<Instant>,
    homing: bool,
    /// Server clock (GameTime and when it arrived), weather number, and the zone's weather keys.
    clock: Option<(u32, Instant)>,
    weather: u16,
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
    map_markers: Vec<event::Marker>,
    event_mem: crate::event_vm::Memory,
    event_end_at: Option<Instant>,
    /// Talk sent, waiting for the server (DAT_10470520).
    talk_lock: Option<Instant>,
    /// Event volume changes for the mixer: (mask, level, ticks).
    audio_cmds: Vec<(i32, f32, f32)>,
    names: Option<Arc<crate::dat::names::Names>>,
    item_info: Option<Arc<items::ItemCache>>,
    msgshift: Option<Arc<crate::dat::msgshift::MsgShift>>,
    zone_text: Option<(u16, Arc<crate::dat::dialog::Dialog>)>,
    my_race: u8,
    inv: items::Inventory,
    win: Option<items::Win>,
    stats: Option<items::CharStats>,
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
    /// Our own settings screen when opened from the Config window, and a request to open it.
    settings: Option<settings::SettingsUi>,
    want_settings: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Confirm {
    Logout,
    Shutdown,
}

impl World {
    fn dead(&self) -> bool {
        matches!(self.my_motion.status, 2 | 3)
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
    /// Wheel notches since the last frame (FOV zoom).
    wheel: f32,
    mods: winit::keyboard::ModifiersState,
    press_at: Option<(f64, f64)>,
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
    /// Title art, buttons and font from the game's lobby DATs; egui stand-ins until loaded.
    game_ui: Option<GameUi>,
    game_ui_state: LoadState,
    /// Whether this frame's UI pass drew game UI (lobby art or the HUD).
    ui_drawn: bool,
    /// Event crossfade and fade passes over the scene.
    screen_fx: Option<crate::render::screenfx::ScreenFx>,
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
    /// The pointer is over the window (edge panning).
    cursor_in: bool,
    /// A lobby or login error waiting in the OK window on the login screen.
    login_error: Option<String>,
    /// The title's Config window (lobycwin) with its values, and our settings screen over the title.
    lobby_cfg: Option<configui::ConfigUi>,
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
            wheel: 0.0,
            mods: Default::default(),
            press_at: None,
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
            game_ui: None,
            game_ui_state: LoadState::Idle,
            ui_drawn: false,
            screen_fx: None,
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
            scene_scale: None,
            cursor_in: false,
            login_error: None,
            lobby_cfg: None,
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
            a.set_volumes(self.cfg.music_volume * m, self.cfg.effects_volume * e);
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
        self.phase = Phase::login(&self.cfg);
        self.lobby_fade = LobbyFade::default();
        if error {
            log::warn!("back to the login screen: {}", self.status);
            self.login_error = Some(self.status.clone());
        }
    }

    /// Retail's levels under ours: in game ids 10 / 9 (FUN_10036480 / FUN_10035060), in the lobby 0xa8 for both
    /// (FUN_10196390).
    fn volume_factors(&self) -> (f32, f32) {
        match &self.phase {
            Phase::World(w) => (w.game.get(10) as f32 / 100.0, w.game.get(9) as f32 / 100.0),
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
                    confirm: confirm.is_some() || (create && self.creator_step == 7),
                    yes: if create { self.creator_yes } else { self.delete_yes },
                    step: if create { self.creator_step } else { 0 },
                    menu_open: false,
                    world: false,
                }
            }
            Phase::World(w) => UiSnap {
                screen: None,
                cursor: vec![w.menu.unwrap_or(99), w.menu_page as usize],
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
            return Some(if b.menu_open { Ui::Open } else if b.confirm { Ui::Select } else { Ui::Cancel });
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
            self.audio = crate::audio::Audio::open(self.cfg.ffxi_dir(), Some(&device), self.cfg.music_volume * m, self.cfg.effects_volume * e);
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
        if let Phase::World(w) = &mut self.phase {
            for (mask, level, ticks) in w.audio_cmds.drain(..) {
                a.event_volume(mask, level, ticks);
            }
            if !event::active(w) {
                w.event_voices = false;
            }
            a.max_se = if w.event_voices { 20 } else { self.cfg.tool("0029").clamp(1, 20) };
            // Interface sounds (targeting, menus, prompts).
            for id in w.sounds.drain(..) {
                a.play_ui_se(id);
            }
            let hour = w.clock.map(|(t, at)| crate::dat::weather::vana_minute(t, at.elapsed().as_secs_f32()) as u32 / 60).unwrap_or(12);
            let cx = crate::audio::MusicContext { status: w.my_motion.status, hour, party: 1 };
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
        if !matches!(task, Task::GameUi(_) | Task::Flyover(_) | Task::Nation(..) | Task::Select(..) | Task::CreatorBackdrop(_) | Task::Dancer(_) | Task::Battle(_) | Task::Names(_) | Task::AreaLoaded(..)) {
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
            Task::AccountCreated(Ok(())) => self.status = "Account created. You can log in now.".into(),
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
                        self.phase = Phase::World(Box::new(World {
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
                            my_id: login.char_id,
                            my_index: 0,
                            pos: Vec3::ZERO,
                            yaw: 0.0,
                            entities: HashMap::new(),
                            zone_lines: Vec::new(),
                            zoning_since: None,
                            chat: Default::default(),
                            chat_input: String::new(),
                            log: Default::default(),
                            hud_top: Default::default(),
                            tells: Vec::new(),
                            battle: None,
                            pending: Vec::new(),
                            action_menu: None,
                            started: Instant::now(),
                            logging_out: false,
                            target: None,
                            locked: false,
                            cam: Default::default(),
                            unlock_ticks: 0.0,
                            camst: camera::State::new(),
                            last_status: 0,
                            auto_lock: true,
                            models: HashMap::new(),
                            models_loading: HashSet::new(),
                            zone_uploading: None,
                            party_tp: self.cfg.party_tp,
                            overlay: None,
                            map_opacity: self.cfg.map_overlay_opacity,
                            target_since: Instant::now(),
                            cam_turn: (0.0, 0.0),
                            sounds: Vec::new(),
                            area_line: None,
                            pos_zone: 0,
                            unsettled: false,
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
                            home_at: None,
                            homing: false,
                            clock: None,
                            weather: 1,
                            zone_weather: None,
                            area: AreaSwitch::default(),
                            event_voices: false,
                            event_looks: Vec::new(),
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
                            map_markers: Vec::new(),
                            event_mem: Default::default(),
                            event_end_at: None,
                            talk_lock: None,
                            audio_cmds: Vec::new(),
                            names: None,
                            item_info: None,
                            msgshift: None,
                            zone_text: None,
                            my_race: 0,
                            inv: Default::default(),
                            win: None,
                            stats: None,
                            npc_names: HashMap::new(),
                            motion_files: Default::default(),
                            game: self.cfg.game.for_character(Some(&login.name)),
                            config: None,
                            volume_dirty: true,
                            game_save: false,
                            settings: None,
                            want_settings: false,
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
                            world.renderer = Some(ZoneRenderer::streamed(&gfx.gpu, geometry, std::mem::take(&mut scene.textures), gfx.config.format));
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
            Task::ZoneLoaded(zone_id, result) => {
                let Phase::World(world) = &mut self.phase else { return };
                world.loading = None;
                world.area = AreaSwitch::default();
                match result {
                    Ok((mut scene, ground, weather, geometry, (zone_fx, global_fx))) => {
                        world.zone_weather = weather;
                        if let Some(g) = global_fx {
                            world.effects.set_global(g);
                        }
                        if let Some(z) = zone_fx {
                            if let Some(keys) = self.keys_table.clone() {
                                world.effects.set_keys(keys);
                            }
                            if let Some(index) = world.index.clone() {
                                world.effects.set_index(index);
                            }
                            world.effects.load_zone(z);
                            world.effects.set_weather(crate::dat::weather::weather_tag(world.weather), 0.0);
                        }
                        world.zone_lines = scene.regions.iter().filter(|r| r.name.starts_with(['z', 'Z']) && r.name.len() == 8).cloned().collect();
                        // Its geometry and textures go up over the next frames; the indicator stays until then.
                        if let Some(gfx) = &self.gfx {
                            world.effects.reg.light_keys = geometry.light_keys.clone();
                            world.renderer = Some(ZoneRenderer::streamed(&gfx.gpu, geometry, std::mem::take(&mut scene.textures), gfx.config.format));
                            world.zone_uploading = Some(Instant::now());
                        }
                        world.ground = Some(ground);
                        world.zone = Some(zone_id);
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
        while let Ok(ev) = world.net.events.try_recv() {
            match ev {
                NetEvent::Zoning => {
                    self.status = "Zoning...".into();
                    world.zoning_since = Some(Instant::now());
                    world.downloading = Some(Instant::now());
                }
                NetEvent::SelfLook(tbl) => looks.push((world.my_id, Source::Pc(look_from(tbl)))),
                NetEvent::ZoneIn { zone, pos, dir, char_id, index, name, look, motion, game_time, weather, music, event: login_event, area } => {
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
                    world.map = None;
                    if let Some(ev) = login_event {
                        event::queue(world, ev);
                    }
                    let _ = world.net.commands.send(NetCommand::StatusRequest(0));
                    world.music.zone_in(music);
                    log::info!("zone music {music:?}");
                    world.clock = Some((game_time, Instant::now()));
                    world.weather = weather;
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
                    let f = forward_from_dir(dir);
                    camera::face(world, f.x.atan2(f.z));
                    world.my_id = char_id;
                    world.name = name;
                    world.entities.clear();
                    if world.zone != Some(zone) && world.loading != Some(zone) {
                        world.loading = Some(zone);
                        load = Some(zone);
                    } else if world.loading.is_none() && world.zone_uploading.is_none() {
                        world.downloading = None;
                    }
                }
                NetEvent::Entity(e) => {
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
                        untargetable: false,
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
                        motion.status = e.motion.status;
                    }
                    motion.graph_size = e.motion.graph_size;
                    let died = !fresh && matches!(motion.status, 2 | 3) && !matches!(entry.motion.status, 2 | 3);
                    entry.motion = motion;
                    entry.index = e.index;
                    if let Some((named, plural)) = e.name_flags {
                        (entry.named, entry.plural) = (named, plural);
                    }
                    if died && let Some(a) = world.actors.get_mut(&e.id) {
                        combat::die(a);
                    }
                    if let Some(p) = e.pos {
                        let mut p = Vec3::from(p);
                        entry.grounded = e.ground;
                        if e.ground
                            && let Some(y) = world.ground.as_ref().filter(|_| world.zone == Some(world.pos_zone)).and_then(|g| g.snap(p.x, p.z, p.y))
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
                    // The zone list wins: LSB sends database names ("Tunnel_Worm").
                    if let Some(n) = world.npc_names.get(&e.id) {
                        if entry.name != *n {
                            entry.name = n.clone();
                        }
                    } else if let Some(n) = e.name {
                        entry.name = n;
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
                    if let Some((b20, b3)) = e.hp_bits {
                        (entry.no_gauge, entry.no_plate) = (b20 || b3, b3);
                    }
                }
                NetEvent::Chat { kind, from, text } => {
                    if kind == 3 && !from.is_empty() {
                        world.tells.retain(|t| *t != from);
                        world.tells.insert(0, from.clone());
                    }
                    let line = stamped(world, chat_line(kind, &from, &text));
                    let c = cfgapply::chat_colour(&world.game, kind);
                    world.chat.push_colour(line, c);
                }
                NetEvent::Logout { state } => {
                    log::info!("logged out of the zone (0x00B state {state})");
                    if world.logging_out {
                        self.status = "Logged out.".into();
                        self.exit_requested = world.shutdown;
                        self.to_login(false);
                        return;
                    }
                }
                NetEvent::Battle(m) => combat::on_battle_message(world, &m),
                NetEvent::Event(s) => event::queue(world, s),
                NetEvent::EventParams(n) => event::params(world, n),
                NetEvent::EventRelease { mode, event: e } => event::release(world, mode, e),
                NetEvent::Wpos { id, pos, dir, mode } => {
                    // POSMODE (LSB): 0 normal, 1 event, 3 pop, 5 reset, 6 materialize move us; 0x0a turns.
                    if id == world.my_id && matches!(mode, 0 | 1 | 3 | 5 | 6 | 0x0a) {
                        if mode != 0x0a {
                            world.pos = Vec3::from(pos);
                        }
                        let f = forward_from_dir(dir);
                        camera::face(world, f.x.atan2(f.z));
                    }
                    event::position_ack(world, mode);
                }
                NetEvent::ZoneText(t) => event::zone_text(world, &t),
                NetEvent::ItemMax(s) => world.inv.sizes = s,
                NetEvent::ItemNum { container, index, count } => world.inv.count(container, index, count),
                NetEvent::Item { container, index, id, count } => world.inv.set(container, index, id, count),
                NetEvent::Equip { slot, index, container } => world.inv.set_equip(slot, index, container),
                NetEvent::CharStats(s) => world.stats = Some(s),
                NetEvent::Action(a) => combat::on_action(world, &a),
                NetEvent::Scheduler { actor, name, .. } => combat::on_scheduler(world, actor, name),
                NetEvent::Status(s) => log::info!("net: {s}"),
                NetEvent::SystemMessage { id, para, para2 } => combat::on_system_message(world, id, [para as i32, para2 as i32]),
                NetEvent::Weather(w, minutes) => {
                    world.weather = w;
                    // One Vana'diel minute is 2.4 s (FUN_101804d0's duration is u16 @0x0a x 3600 units).
                    world.effects.set_weather(crate::dat::weather::weather_tag(w), minutes as f32 * 2.4);
                }
                NetEvent::Assist { player, target: t } => target::assist(world, player, t),
                NetEvent::KeyItems { table, bits } => {
                    let at = table as usize * 16;
                    if world.key_items.len() < at + 16 {
                        world.key_items.resize(at + 16, 0);
                    }
                    world.key_items[at..at + 16].copy_from_slice(&bits);
                }
                NetEvent::Music { slot, id } => world.music.set_slot(slot, id),
                NetEvent::SelfStatus { status, hpp, home_in, pet } => {
                    world.my_pet = pet;
                    // The server prints the defeat (0x029 #97 / #20).
                    if matches!(status, 2 | 3) && !world.dead() {
                        if let Some(a) = world.actors.get_mut(&world.my_id) {
                            combat::die(a);
                        }
                    }
                    world.my_motion.status = status;
                    world.home_at = home_in.map(|s| Instant::now() + std::time::Duration::from_secs(s as u64));
                    if home_in.is_none() {
                        world.homing = false;
                    }
                    if let Some(v) = &mut world.vitals {
                        v.3 = hpp;
                    }
                }
                NetEvent::Vitals { hp, mp, tp, hpp, mpp } => world.vitals = Some((hp, mp, tp, hpp, mpp)),
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
            let actor = world.actors.entry(id).or_insert_with(Actor::new);
            if actor.key == key {
                continue;
            }
            actor.key = key.clone();
            actor.source = Some(source);
            // A look already loaded is shared at once; one load serves every actor waiting for it.
            if let Some(m) = world.models.get(&key) {
                actor.character = Some(m.clone());
                actor.gpu = None;
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
        if let Some(zone_id) = load {
            self.status = format!("Loading zone {zone_id}...");
            let need_global = match &self.phase {
                Phase::World(w) => !w.effects.has_global(),
                _ => false,
            };
            match self.game_data() {
                Ok((index, keys)) => self.spawn(move || {
                    Task::ZoneLoaded(zone_id, (|| {
                        let file = index.read(crate::dat::zone::file_id(zone_id as u32))?;
                        let scene = zone::load_scene(&keys, &file)?;
                        let ground = ground::Ground::build(&scene);
                        let weather = crate::dat::weather::ZoneWeather::parse(&file).map_err(|e| log::warn!("zone {zone_id} weather: {e:#}")).ok();
                        let geometry = Arc::new(crate::render::ZoneGeometry::build(&scene));
                        let global = need_global.then(|| index.read(0).and_then(|f| crate::fx::lib::Library::load(0, f))).and_then(|r| r.map_err(|e| log::warn!("effects: DAT 0: {e:#}")).ok());
                        let zone_fx = crate::fx::lib::Library::load(crate::dat::zone::file_id(zone_id as u32), file).map_err(|e| log::warn!("effects: zone {zone_id}: {e:#}")).ok().map(|mut l| {
                            l.prewarm(&keys);
                            l
                        });
                        Ok((scene, ground, weather, geometry, (zone_fx, global)))
                    })())
                }),
                Err(e) => self.status = format!("{e:#}"),
            }
        }
    }

    fn update_movement(&mut self, dt: f32) {
        let Phase::World(world) = &mut self.phase else { return };
        combat::tick(world);
        event::update(world, dt);
        // Weapon-skill motion files and motions the effect system's schedulers asked for last frame.
        for (id, files) in std::mem::take(&mut world.effects.motion_files) {
            if let Some(a) = world.actors.get_mut(&id) {
                a.pending = Some(actmotion::Pending { files, name: String::new(), kind: actmotion::Kind::Skill });
            }
        }
        for (id, m) in std::mem::take(&mut world.effects.motions) {
            actmotion::play_op(world, id, m);
        }
        actmotion::update(world, dt);
        // Inferred safety: a talk the server never answers stops blocking after 10 s.
        if world.talk_lock.is_some_and(|t| t.elapsed().as_secs_f32() > 10.0) {
            world.talk_lock = None;
        }
        let in_event = event::active(world);
        target::update(world);
        combat::area_line(world);
        let ticks = dt * 60.0;
        // FUN_1008dfb0 / FUN_100a3560: grounded actors follow the collision floor every frame (also those that
        // arrived before the zone had loaded).
        let floor = world.ground.as_ref().filter(|_| world.zone == Some(world.pos_zone) && !in_event);
        for e in world.entities.values_mut() {
            // Moved by someone else since last frame (an event script): take that as the slide and drawn spot.
            if e.pos != e.last_pos {
                e.slide = e.pos;
                e.remain = 0.0;
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
            // A step under 0.01 a tick counts as standing (motion and facing).
            let flat = Vec3::new(step.x, 0.0, step.z);
            let moved = if flat.length() < 0.01 * ticks { 0.0 } else { flat.length() };
            let want = if moved > 0.0 { (-flat.z).atan2(flat.x) } else { event::rad_of(e.dir) };
            let tau = std::f32::consts::TAU;
            let diff = (want - e.yaw + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI;
            e.yaw += diff * (1.0 - 0.875f32.powf(ticks));
            e.ratio = crate::motion::move_ratio(e.ratio, moved, dt, e.motion.speed, e.motion.speed_base, e.motion.status);
            e.last_pos = e.pos;
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
        // Black while zoning or loading; fade back in once we stand in the new zone.
        let ready = world.zoning_since.is_none() && world.zone == Some(world.pos_zone) && world.renderer.is_some();
        let target = if ready && !world.unsettled { 0.0 } else { 1.0 };
        if target > world.fade {
            world.fade = (world.fade + dt / 0.4).min(1.0);
        } else if target < world.fade {
            world.fade = (world.fade - dt / 0.6).max(0.0);
        }
        if world.unsettled && world.zone == Some(world.pos_zone) {
            if let Some(y) = world.ground.as_ref().and_then(|g| g.settle(world.pos.x, world.pos.z, world.pos.y)) {
                log::info!("settled from y {:.2} to {:.2}", world.pos.y, y);
                world.pos.y = y;
            }
            world.unsettled = false;
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
        if world.autorun && !world.locked {
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
        }
        // FUN_100a5f60: |v| > 0.9 full speed, 0.05..0.9 a third; walking caps at a third.
        let mag = if world.autorun && !world.locked { 1.0 } else { ax.hypot(ay) };
        let mut tier = if mag > 0.9 { 1.0 } else if mag >= 0.05 { 1.0 / 3.0 } else { 0.0 };
        if world.walk {
            tier = f32::min(tier, 1.0 / 3.0);
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
            let (dx, dy) = std::mem::take(&mut world.cam_turn);
            target::step_camera(world, dt * 60.0, dx + turn_keys * 2.0 * dt, dy);
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
        let falls = !in_event && !world.unsettled && world.zone == Some(world.pos_zone);
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
            let g = world.ground.as_ref().filter(|g| !g.is_empty());
            if stepping {
                let s = step.normalize();
                let d = speed * tier * ground::STEP_TICKS / 60.0;
                let next = match lock_target {
                    Some(tp) => target::locked_step(from, tp, s.dot(forward), s.dot(right), d),
                    None => from + s * d,
                };
                world.pos = g.map_or(next, |g| g.walk_player(from, next, ground::STEP_TICKS));
            } else if falls && let Some(g) = g {
                // A fall goes on without input.
                world.pos = g.walk_player(from, from, ground::STEP_TICKS);
            }
            world.step_hist = [world.step_hist[1], world.step_hist[2], world.step_hist[3], world.pos];
        }
        world.step_to = world.pos;
        world.draw_pos = drawn_between_steps(&world.step_hist, world.step_ticks / ground::STEP_TICKS);
        if *TRACE.get_or_init(|| std::env::var_os("FFXI_TRACE_MOVE").is_some()) && stepping {
            let c = Self::camera(world);
            log::info!("me t {:.4} steps {steps} y {:.4} drawn ({:.3}, {:.4}, {:.3}) eye ({:.3}, {:.4}, {:.3}) look ({:.3}, {:.4}, {:.3})", world.started.elapsed().as_secs_f32(), world.pos.y, world.draw_pos.x, world.draw_pos.y, world.draw_pos.z, c.eye.x, c.eye.y, c.eye.z, c.target.x, c.target.y, c.target.z);
        }
        camera::step(world, dt, &cam_in);
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
            self.input.context = if w.map.is_some() { 0x26 } else { 0xffff };
            f.typing = w.typing;
            f.keyboard = w.game.get(0x42);
            f.window = w.menu.is_some() || w.confirm.is_some() || w.win.is_some() || w.action_menu.is_some() || w.config.is_some() || w.settings.is_some() || w.map.is_some() || w.full_log.is_some();
            f.assigning = w.config.as_ref().and_then(|c| c.stack.last()).is_some_and(|t| t.name == "k1assign" || t.name == "k2assign");
        }
        self.input.begin_frame(f);
        let Phase::World(world) = &mut self.phase else { return };
        // Numpad * / compact H: 0x44 (/heal) and 0x30 (lock-on) read the same press; FUN_101516d0 picks.
        if self.input.test(0x44, input::mode::PRESS) {
            target::numpad_star(world);
        }
        // Numpad + / Ctrl or Alt + F / compact F, mode 0 (repeats): FUN_10151550.
        if self.input.test(0x78, input::mode::REPEAT) {
            log_cycle(world);
        }
    }

    fn camera(world: &World) -> Camera {
        event::camera(world).unwrap_or_else(|| camera::view(world))
    }

    /// The entity under a cursor position, nearest first: each is a body-sized screen box.
    fn pick(world: &World, cursor: (f64, f64), size: [u32; 2], aspect: f32) -> Option<u32> {
        let vp = Self::camera(world).view_proj(aspect);
        let hidden = event::hidden(world);
        let to_screen = |p: Vec3| {
            let c = vp * p.extend(1.0);
            (c.w > 0.1).then(|| (((c.x / c.w + 1.0) / 2.0 * size[0] as f32), ((1.0 - c.y / c.w) / 2.0 * size[1] as f32), c.w))
        };
        let (mx, my) = (cursor.0 as f32, cursor.1 as f32);
        world
            .entities
            .iter()
            .filter(|(id, e)| !e.name.starts_with('_') && e.targetable() && !hidden.contains(id))
            .filter_map(|(id, e)| {
                let (fx, fy, depth) = to_screen(e.pos)?;
                let (_, hy, _) = to_screen(e.pos + Vec3::new(0.0, -2.0, 0.0))?;
                let half_w = ((fy - hy).abs() * 0.25).max(6.0);
                (mx >= fx - half_w && mx <= fx + half_w && my >= hy.min(fy) && my <= hy.max(fy)).then_some((*id, depth))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Tab targeting: the next entity by distance after the current target, within 30 units.
    fn cycle_target(world: &mut World) {
        if !target::picks_allowed(world) {
            return;
        }
        let hidden = event::hidden(world);
        let mut near: Vec<(u32, f32)> = world
            .entities
            .iter()
            .filter(|(id, e)| !e.name.is_empty() && !e.name.starts_with('_') && e.targetable() && !hidden.contains(id))
            .map(|(id, e)| (*id, (e.pos - world.pos).length()))
            .filter(|(_, d)| *d < 30.0)
            .collect();
        near.sort_by(|a, b| a.1.total_cmp(&b.1));
        let next = match world.target.and_then(|t| near.iter().position(|(id, _)| *id == t)) {
            Some(i) => near.get(i + 1).or(near.first()),
            None => near.first(),
        };
        let t = next.map(|(id, _)| *id);
        target::set(world, t);
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
        let input = ctx.input(|i| UiInput {
            pointer: i.pointer.hover_pos().map(|p| (p.x * ppp * k.0, p.y * ppp * k.1)),
            moved: i.pointer.delta() != egui::Vec2::ZERO,
            clicked: i.pointer.primary_clicked(),
            events: i.events.clone(),
        });
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
                egui::Area::new(egui::Id::new("error")).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(format!("The game data could not be loaded.\n{status}"));
                });
            }
            return;
        }
        // Screen changes and leaving the lobby wait for the fade-out (LobbyFade); input meanwhile is dropped.
        let before = if let Phase::Characters { screen, .. } = &self.phase { Some(*screen) } else { None };
        let fading = self.lobby_fade.next.is_some();
        let mut fade_action = false;
        match &mut self.phase {
            Phase::Login { password, otp, focus } => {
                if let Some(lu) = self.game_ui.as_mut() {
                    self.ui_drawn = true;
                    let fields = [&mut self.cfg.server, &mut self.cfg.username, &mut *password, &mut *otp];
                    // While an error shows, the fields take no input (the OK window has it).
                    let none = UiInput { pointer: None, moved: false, clicked: false, events: Vec::new() };
                    let shown = if self.login_error.is_some() { &none } else { &input };
                    let chosen = login_window(lu, shown, size, fields, focus, busy, &status, &self.cfg.client_version);
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
                            let slots = lu.title_update(sw, sh, Some(shown), logos, child_enabled, upd.then_some((*menu == UPDATE, true)), wardrobes);
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
                                    if let egui::Event::Key { key, pressed: true, .. } = ev {
                                        closed |= s.key(*key);
                                        if let Some(a) = self.audio.as_mut() {
                                            for x in s.sounds.drain(..) {
                                                a.ui(x);
                                            }
                                        }
                                    }
                                }
                                if let Some(p) = input.pointer.filter(|_| !settings_new) {
                                    s.pointer(p, input.moved, input.clicked, sw, sh);
                                    if let Some(a) = self.audio.as_mut() {
                                        for x in s.sounds.drain(..) {
                                            a.ui(x);
                                        }
                                    }
                                }
                                if s.dirty {
                                    s.dirty = false;
                                    s.s.store(&mut self.cfg);
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
                        // FUN_10235ec0 case 5: a disabled button (Create excepted) only buzzes.
                        if chosen.is_some_and(|i| i < 5 && i != 1 && !enabled[i]) {
                            if let Some(a) = self.audio.as_mut() {
                                a.ui(crate::audio::Ui::Error);
                            }
                            chosen = None;
                        }
                        match chosen {
                            Some(UPDATE) => {
                                if let Some(s) = self.updates.clone() {
                                    self.update_ui = Some(updater::UpdateUi::open(s, self.cfg.ffxi_dir()));
                                    if let Some(a) = self.audio.as_mut() {
                                        a.ui(crate::audio::Ui::Select);
                                    }
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
                        if escape && !config_open {
                            action = Some(Box::new(|app: &mut App| app.phase = Phase::login(&app.cfg)));
                            fade_action = true;
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
                                *screen = LobbyScreen::Main;
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
                    egui::Area::new(egui::Id::new("status")).anchor(egui::Align2::LEFT_TOP, [8.0, 56.0]).interactable(false).show(ctx, |ui| {
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
                if std::mem::take(&mut world.want_settings) {
                    let mut s = settings::SettingsUi::open(&self.cfg, screen_size);
                    s.monitors = self.monitors.clone();
                    s.gpus = self.gpus.clone();
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
            // A script that logs in walks the title menu itself; pick the character once it reaches Select.
            Phase::Characters { lobby, chars, screen, .. } if !self.drive.input.as_ref().is_some_and(|s| s.logs_in) || *screen == LobbyScreen::Select => {
                if let (Some(name), Some(_)) = (self.drive.character.clone(), lobby.as_ref()) {
                    if let Some(c) = chars.iter().find(|c| c.name.eq_ignore_ascii_case(&name)).cloned() {
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
            Phase::World(w) if w.renderer.is_some() && w.zone_uploading.is_none() && w.fade == 0.0 && w.zone == Some(w.pos_zone) && w.event_pending.is_none() => "world",
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
                            (Look { race: c.race as u8, face: c.face, head: l[1], body: l[2], hands: l[3], legs: l[4], feet: l[5], main: l[6], sub: l[7] }, c.size)
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
            // Test drives only: target the nearest living monster with a model (Tab picks NPCs first at gates).
            if let (Some("target-mob"), Phase::World(world)) = (action.as_deref(), &mut self.phase) {
                let pos = world.pos;
                let mob = world.entities.iter().filter(|(id, e)| e.mob && !matches!(e.motion.status, 2 | 3) && world.actors.get(id).is_some_and(|a| a.gpu.is_some())).min_by(|a, b| (a.1.pos - pos).length().total_cmp(&(b.1.pos - pos).length())).map(|(id, _)| *id);
                target::set(world, mob);
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
        let shoot = shoot || script_shot.is_some();
        let ctx = gfx.egui_ctx.clone();
        let before = self.ui_snap();
        let output = perf::timed(|| "egui".into(), || ctx.run_ui(raw, |ui| self.ui(ui.ctx(), size)));
        let after = self.ui_snap();
        if let (Some(s), Some(a)) = (Self::ui_sound(&before, &after), self.audio.as_mut()) {
            a.ui(s);
        }
        perf::timed(|| "audio".into(), || self.update_audio(dt));
        gfx.egui_state.handle_platform_output(&gfx.window, output.platform_output);
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
        let scaled = bg != size || gamma.iter().any(|g| (g - 1.0).abs() > 1e-3);
        let (scene_view, scene_depth, scene_tex) = if scaled {
            self.scene_scale.get_or_insert_with(|| crate::render::scale::SceneScale::new(&gfx.gpu, gfx.config.format)).target(&gfx.gpu, bg)
        } else {
            (view.clone(), gfx.depth.clone(), frame.texture.clone())
        };
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
            let idles = event::idles(w);
            // Placements first (look targets read other actors), then each actor's motion and pose.
            let my_dir = w.net.me.lock().map(|m| m.dir).unwrap_or(0);
            let places: HashMap<u32, Place> = w.actors.keys().filter_map(|id| actor_place(w, *id, my_dir).map(|p| (*id, p))).collect();
            let look_points: HashMap<u32, Vec3> = w
                .actors
                .iter()
                .filter_map(|(id, a)| match a.look.mode {
                    actmotion::LookMode::Actor(t, k) => attach_point(w, &places, t, k).map(|p| (*id, p)),
                    _ => None,
                })
                .collect();
            // Foot points that came back from earlier frames: steps (FUN_100d2970).
            if let Some(pts) = w.renderer.as_mut().map(|r| r.points.take()) {
                footsteps::steps(w, &places, pts);
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
                    r.lighting = zw.at(crate::dat::weather::weather_tag(weather), minute).map(|e| crate::dat::weather::Lighting::from_env(&e, minute));
                    w.area_lights = zw.areas.iter().filter_map(|(&id, a)| Some((id, a.at(crate::dat::weather::weather_tag(weather), minute)?.area_light()))).collect();
                }
                // Op 0x10: the frame before the cut, drawn once offscreen from the camera it had (last frame's
                // poses; its own submit so the zone uniforms keep that camera).
                if let Some(cr) = w.screen.cross.filter(|c| !c.captured) {
                    if let Some(cam) = cr.camera {
                        let fx = self.screen_fx.get_or_insert_with(|| crate::render::screenfx::ScreenFx::new(&gfx.gpu, gfx.config.format));
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
                for (id, a) in w.actors.iter_mut() {
                    let Some(ch) = a.character.clone() else { continue };
                    let Some(&Place { pos, forward, status, ratio, size }) = places.get(id) else { continue };
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
                        (None, Some(m)) => (m.name.clone(), m.speed, m.count == 1, m.blend.max(1) as f32),
                        // Op 0x5e / 0x6b: the event's idle motion while the actor stands.
                        (None, None) if ratio < 1e-3 && idles.get(id).is_some_and(|n| has(n)) => (idles[id].clone(), 1.0, false, crate::motion::BLEND_TICKS),
                        (None, None) => {
                            let (n, r) = crate::motion::choose(&ch.motions, status, ratio);
                            (n.to_string(), r, false, crate::motion::BLEND_TICKS)
                        }
                    };
                    if name != a.motion || (once && a.oneshot.as_ref().is_some_and(|o| o.ticks == 0.0)) {
                        if !a.motion.is_empty() {
                            a.prev = Some((std::mem::take(&mut a.motion), a.ticks));
                            a.blend = 0.0;
                        }
                        a.blend_len = blend;
                        a.motion = name.clone();
                        a.ticks = 0.0;
                    }
                    a.ticks += dt * 60.0 * rate;
                    if let Some(o) = &mut a.oneshot {
                        o.ticks = a.ticks;
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
                    let mut anims = frames_of(&name, a.ticks, once);
                    // The mouth motion of a spoken line goes over the body motion (its face tracks win).
                    if let Some(m) = &mut a.mouth
                        && let Some(mt) = m.ticks
                    {
                        let mouth: Vec<_> = ["mou4", "mou6"].iter().map(|n| actmotion::find_cached(&mut cache.borrow_mut(), &extra, &ch, n)).find(|v| !v.is_empty()).unwrap_or_default();
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
                    a.blend += dt * 60.0;
                    if a.blend >= a.blend_len {
                        a.prev = None;
                    }
                    // Motions are evaluated and skinned on the GPU (render/anim.rs, vs_skinned).
                    let from = a.prev.as_ref().map(|(old, old_ticks)| frames_of(old, *old_ticks, false)).unwrap_or_default();
                    a.find_cache = cache.take();
                    let t = a.blend / a.blend_len;
                    a.scale = ch.scale(size);
                    // The rest pose's mesh top, once (stub models, plates without attach points).
                    if a.top_key.is_empty() {
                        a.rest = crate::dat::character::pose(&ch.skeleton, &[]);
                        a.top = crate::render::character::skinned_top(&ch, &a.rest) * a.scale;
                        a.top_key = "rest".into();
                    }
                    // CPU bones only where the locked camera reads them (player bone 13, target bone 5).
                    if w.locked && (*id == w.my_id || w.target == Some(*id)) {
                        a.bones = if from.is_empty() { crate::dat::character::pose(&ch.skeleton, &anims) } else { crate::render::character::pose_blend(&ch, &from, &anims, t) };
                    }
                    let model = actor_matrix(pos, forward) * glam::Mat4::from_scale(Vec3::splat(a.scale));
                    // FUN_100d38c0: the look point in actor space (x forward), eased and weighted.
                    let target = match a.look.mode {
                        actmotion::LookMode::Actor(..) => look_points.get(id).map(|p| model.inverse().transform_point3(*p)),
                        actmotion::LookMode::Facing(pitch, yaw) => {
                            let (p, y) = (pitch as f32 * TAU_4096, yaw as f32 * TAU_4096);
                            let head = a.rest.get(ch.skeleton.points.get(7).map_or(0, |q| q.0 as usize)).map_or(Vec3::ZERO, |b| b.1);
                            Some(head + Vec3::new(p.cos() * y.cos(), -p.sin(), -p.cos() * y.sin()) * 20.0)
                        }
                        actmotion::LookMode::Off => None,
                    };
                    a.look.step(target, dt * 60.0);
                    let look = (a.look.weight > 0.0).then(|| crate::render::anim::HeadLook {
                        bones: [3usize, 7].map(|k| ch.skeleton.points.get(k).map(|p| p.0)),
                        weight: a.look.weight,
                        point: a.look.point,
                    });
                    let drawn = !a.vanished() && !hidden.contains(id) && !(*id == w.my_id && w.camst.self_alpha <= 0.0);
                    let g = a.gpu.as_mut().unwrap();
                    g.alpha = (a.alpha / actmotion::OPAQUE).clamp(0.0, 1.0) * if *id == w.my_id { w.camst.self_alpha } else { 1.0 };
                    // The flash runs only while the actor is drawn, in 60 Hz ticks whatever the frame rate.
                    g.flash = if drawn { a.flash.step(w.target == Some(*id), dt * 60.0) } else { flash::NONE };
                    // The floor's lights (retail copies them at the ground probe, every 121 updates).
                    g.lights = w.ground.as_ref().map_or([0; 4], |gr| gr.lights(pos));
                    if drawn {
                        let files: Vec<_> = extra.iter().map(|(_, f)| f.clone()).collect();
                        r.pose_actor_gpu(g, &ch, &files, &anims, if a.prev.is_some() { &from } else { &[] }, t, model, look);
                        counts.posed += 1;
                        if footsteps::wanted(&ch, status, pos, camera.eye) {
                            for (slot, &k) in footsteps::POINTS.iter().enumerate().take(if ch.cib[0] == 2 { 4 } else { 2 }) {
                                if let Some(&(bone, off)) = ch.skeleton.points.get(k) {
                                    r.query_point(g, footsteps::key(*id, slot), bone as u16, off);
                                }
                            }
                        }
                    }
                }
                r.upload_poses(&gfx.gpu, &mut encoder);
                let actors: Vec<&crate::render::ActorGpu> = w.actors.iter().filter(|(id, a)| !a.vanished() && !hidden.contains(id)).filter_map(|(_, a)| a.gpu.as_ref()).collect();
                counts = perf::Counts { entities: w.entities.len(), actors: w.actors.len(), posed: counts.posed, drawn: actors.len(), zone_draws: r.draw_count(), ui_vertices: 0 };
                m_actors = Instant::now();
                // Mips of new textures (models that came this frame, a zone going up) under the frame budget.
                perf::timed(|| "mips".into(), || gfx.gpu.run_mips(&mut encoder, crate::render::MIP_BUDGET));
                perf::timed(|| "zone render".into(), || r.render(&gfx.gpu, &mut encoder, &scene_view, &scene_depth, &camera, aspect, &actors));
                // Effect generators: CPU emission, the GPU update pass, then the elements over the scene.
                // Its pipelines are made while the zone still goes up (behind the loading indicator), not on the
                // first shown frame.
                let eg = self.effects_gpu.get_or_insert_with(|| crate::render::effects::EffectsGpu::new(&gfx.gpu, gfx.config.format));
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
                    let fa = FxActors { actors: &w.actors, places: &places, contacts: &w.contacts, entities: &w.entities, me: w.my_id, pet: w.my_pet, areas: &w.area_lights, ground: w.ground.as_ref() };
                    if let Some(g) = &w.ground {
                        w.effects.set_covered(g.covered(camera.eye));
                    }
                    if let Some(a) = &self.audio {
                        w.effects.set_alive(a.alive());
                    }
                    w.effects.filter = (w.game.get(0x92) as u32, matches!(w.my_motion.status, 1 | 3));
                    perf::timed(|| "effects".into(), || w.effects.tick(&f, &fa));
                    // Effect sounds (fx/sound.rs) to the sound thread; without audio they are dropped.
                    for c in std::mem::take(&mut w.effects.sound_cmds) {
                        let Some(a) = &mut self.audio else { break };
                        play_fx_sound(a, c);
                    }
                    eg.set_lights(&r.lights);
                    perf::timed(|| "effects upload".into(), || eg.update(&gfx.gpu, &mut encoder, &mut w.effects, &f));
                    let (fog_color, fog) = r.lighting.as_ref().map_or(([0.0; 3], [150.0, 600.0]), |l| (l.fog.to_array(), [l.fog_start, l.fog_end]));
                    eg.draw(&gfx.gpu, &mut encoder, (&scene_view, &scene_depth, bg), Some(&scene_tex), &w.effects, (camera.proj(aspect), camera.view(), camera.eye), [crate::render::NEAR, crate::render::FAR], fog_color, fog);
                }
                // Event crossfade and fades over the scene, under the 2D layer; the zoning fade as a multiply too.
                let (cross, m) = (w.screen.cross_alpha(), w.screen.multiply().map(|c| c * (1.0 - w.fade)));
                if cross.is_some() || m.iter().any(|c| (c - 1.0).abs() > 0.005) {
                    let fx = self.screen_fx.get_or_insert_with(|| crate::render::screenfx::ScreenFx::new(&gfx.gpu, gfx.config.format));
                    fx.draw(&gfx.gpu, &mut encoder, &scene_view, bg, cross, m);
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
        if self.ui_drawn
            && let Some(lu) = &mut self.game_ui
        {
            counts.ui_vertices = lu.vertex_count();
            lu.scene_depth = drew_world.then(|| scene_depth.clone());
            perf::timed(|| "ui render".into(), || lu.render(&gfx.gpu, gfx.config.format, &mut encoder, &view, self.cfg.ui_size(size), drew_world, ui_level, scene_level));
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
        }
        if let (Some(buf), Some(path)) = (capture, script_shot.or_else(|| self.drive.screenshot.clone())) {
            if let Err(e) = save_capture(&gfx.gpu, &buf, size, row, gfx.config.format, &path) {
                log::error!("screenshot failed: {e:#}");
            } else {
                log::info!("screenshot written to {}", path.display());
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
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(Box::new(event_loop.owned_display_handle())));
        let surface = instance.create_surface(window.clone()).expect("creating the surface");
        self.gpus = crate::render::adapter_names(&instance, Some(&surface));
        let gpu = Gpu::with_adapter(instance, Some(&surface), &self.cfg.gpu).expect("opening the GPU");
        let size = window.inner_size();
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
        // A driven run takes its input from the script only; real keys and the mouse are ignored.
        if self.drive.driven()
            && matches!(event, WindowEvent::KeyboardInput { .. } | WindowEvent::ModifiersChanged(_) | WindowEvent::Ime(_) | WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. } | WindowEvent::CursorMoved { .. })
        {
            return;
        }
        let consumed = gfx.egui_state.on_window_event(&gfx.window, &event).consumed;
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                log::info!("window resized to {}x{}", size.width, size.height);
                resize_surface(gfx, size);
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                if (self.drive.quit && self.drive_done() && !matches!(self.phase, Phase::World(_))) || self.exit_requested {
                    event_loop.exit();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
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
            WindowEvent::CursorEntered { .. } => self.cursor_in = true,
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
                        a.set_volumes(self.cfg.music_volume * m, self.cfg.effects_volume * e);
                    } else {
                        a.set_volumes(0.0, 0.0);
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } if !consumed || state == ElementState::Released => {
                let pressed = state == ElementState::Pressed;
                match button {
                    MouseButton::Left => {
                        if pressed {
                            self.press_at = self.last_cursor;
                        } else if let (Some(a), Some(b), Phase::World(world)) = (self.press_at.take(), self.last_cursor, &mut self.phase) {
                            // A click (not a drag) selects whatever is under the cursor.
                            let on_hud = self.hud_blockers.iter().any(|r| inside(r, (b.0 as f32 * self.ui_k.0, b.1 as f32 * self.ui_k.1)));
                            if (a.0 - b.0).abs() + (a.1 - b.1).abs() < 5.0 && !consumed && !on_hud && !event::active(world) {
                                let size = [gfx.config.width, gfx.config.height];
                                let aspect = cfgapply::aspect(size, self.cfg.retail_aspect, world.game.get(0x43));
                                // Key 0x8b (FUN_101524c0): no pick while locked (FUN_10151f20); a click on nothing keeps the
                                // target (FUN_10151d20). A click that leaves the target as it was is the confirm key on it
                                // (FUN_101524e0): talk to an NPC *(inf: monsters take no action here)*.
                                let picked = if target::picks_allowed(world) { Self::pick(world, b, size, aspect) } else { None };
                                match picked.or(world.target) {
                                    Some(t) if world.target == Some(t) => {
                                        if t >> 24 != 0 && world.entities.get(&t).is_some_and(|e| !e.mob) {
                                            event::talk(world, t);
                                        }
                                    }
                                    t => target::set(world, t),
                                }
                            }
                        }
                        self.left_down = pressed;
                    }
                    MouseButton::Right => self.dragging = pressed,
                    MouseButton::Middle => self.middle_down = pressed,
                    _ => {}
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
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
        assert_eq!(chat_line(4, "Djarum", "hi"), "(Djarum) hi");
        assert_eq!(chat_line(3, "Djarum", "hi"), "Djarum>> hi");
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
