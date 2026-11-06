//! LAN voice chat's window (ours, not retail; hidden: Ctrl+Alt+Shift+V in the world opens and closes it) and the
//! list of who is talking. Both are Lua window models (client/lua/window.rs), so each UI draws them as it draws a
//! mod's windows. The engine is crate::voice.

use super::World;
use super::game_ui::GameUi;
use super::lua::window::{Col, Elem, Event, Meta, Row, Window};
use crate::audio::Ui as Se;
use crate::voice::{self, PTT_KEYS, Settings, Talk};
use std::sync::mpsc::Receiver;

/// Voice activity's threshold slider: -70 .. -10 dBFS in 2 dB steps.
const DB_LOW: i32 = -70;
const DB_STEP: i32 = 2;
const DB_STEPS: usize = 31;
const CHANNEL_MAX: usize = 24;

pub struct VoiceChat {
    pub cfg: Settings,
    /// The settings changed: the app saves them.
    pub dirty: bool,
    engine: Option<voice::Engine>,
    /// Why the engine could not start (it is tried again when a setting changes).
    failed: Option<String>,
    /// The game's output device (FFXI_AUDIO_DEVICE first).
    output: String,
    pub win: Window,
    talk: Window,
    status: voice::Status,
    devices: Option<Vec<(String, String)>>,
    listing: Option<Receiver<Vec<(String, String)>>>,
    /// Where the classic UI drew the windows last frame.
    places: std::cell::Cell<[Option<(f32, f32)>; 2]>,
}

impl VoiceChat {
    pub fn new(cfg: Settings, output: String) -> Self {
        let mut win = Window::new("voice");
        (win.title, win.width) = ("Voice Chat".into(), 400.0);
        win.help = "Voice chat with players on this local network. Escape closes the window.".into();
        let mut talk = Window::new("voice talk");
        (talk.hud, talk.framed) = (true, false);
        Self { cfg, dirty: false, engine: None, failed: None, output, win, talk, status: Default::default(), devices: None, listing: None, places: Default::default() }
    }

    /// Whether the window has the keys.
    pub fn focused(&self) -> bool {
        self.win.open
    }

    pub fn typing(&self) -> bool {
        self.win.open && self.win.typing()
    }

    pub fn focused_help(&self) -> Option<(String, String)> {
        self.focused().then(|| (self.win.title.clone(), self.win.help_now().to_string()))
    }

    fn elems(&self) -> Vec<(Elem, &'static str)> {
        let c = &self.cfg;
        let mut v = Vec::new();
        let onoff = |id: &str, label: &str, items: [&str; 2], on: bool| Elem::Buttons { id: id.into(), label: label.into(), items: items.iter().map(|s| s.to_string()).collect(), marked: Some(if on { 0 } else { 1 }) };
        v.push((onoff("on", "Voice Chat", ["On", "Off"], c.enabled), "Turns voice chat on or off. It reaches only this local network."));
        let state = match (&self.engine, &self.failed) {
            _ if !c.enabled => "Off.".to_string(),
            (Some(e), _) => {
                let here = self.status.peers.iter().filter(|p| p.same).count();
                let mut s = match here {
                    0 => "On. No one else is on this channel yet.".to_string(),
                    1 => "On. 1 other player on this channel.".to_string(),
                    n => format!("On. {n} other players on this channel."),
                };
                if let Err(m) = e.mic() {
                    s.push_str(&format!(" No microphone: {m}."));
                }
                if let Err(m) = &e.discovery {
                    s.push_str(&format!(" Finding others on port {} failed ({m}): add them by address.", voice::PORT));
                }
                s
            }
            (None, Some(f)) => format!("Could not start: {f}."),
            (None, None) => "Starting...".to_string(),
        };
        v.push((Elem::Text { id: "state".into(), text: state, dim: true, colour: None }, ""));
        v.push((onoff("mic", "Microphone", ["On", "Muted"], !c.mic_muted), "Mutes your microphone and closes it."));
        v.push((onoff("sound", "Voices", ["On", "Off"], !c.deafened), "Stops you hearing anyone."));
        v.push((Elem::Buttons { id: "talk".into(), label: "Talk".into(), items: vec!["Voice Activity".into(), "Push to Talk".into()], marked: Some((c.talk == Talk::Push) as usize) }, "Voice Activity sends when you speak above the sensitivity; Push to Talk while the key is held."));
        match c.talk {
            Talk::Push => {
                let at = PTT_KEYS.iter().position(|k| k.0 == c.ptt_key).unwrap_or(0);
                v.push((Elem::Choice { id: "ptt".into(), label: "Push-to-Talk Key".into(), items: PTT_KEYS.iter().map(|k| k.1.to_string()).collect(), at }, "Hold it to talk. The game still sees the key."));
            }
            Talk::Voice => {
                let at = ((c.threshold_db - DB_LOW) / DB_STEP).clamp(0, DB_STEPS as i32 - 1) as usize;
                v.push((Elem::Slider { id: "threshold".into(), label: "Sensitivity".into(), n: DB_STEPS, at, shown: format!("{} dB", c.threshold_db) }, "The level your voice must reach to be sent; lower picks up quieter speech (and more noise)."));
            }
        }
        let level = self.status.level_db.max(-100.0);
        let shown = match self.engine.as_ref().map(|e| e.mic()) {
            None => String::new(),
            Some(Err(_)) => "none".to_string(),
            Some(Ok(None)) => "off".to_string(),
            Some(Ok(Some(_))) if self.status.talking => format!("{level:.0} dB  sending"),
            Some(Ok(Some(_))) => format!("{level:.0} dB"),
        };
        v.push((Elem::Gauge { id: "level".into(), label: "Microphone Level".into(), value: ((level - DB_LOW as f32) / -DB_LOW as f32).clamp(0.0, 1.0), text: shown, kind: if self.status.talking { "tp".into() } else { "mp".into() } }, ""));
        v.push((Elem::Slider { id: "gain".into(), label: "Microphone Volume".into(), n: 21, at: (c.mic_gain / 10).min(20) as usize, shown: format!("{}%", c.mic_gain) }, "Your voice before it is sent."));
        v.push((onoff("noise", "Noise Suppression", ["On", "Off"], c.noise_suppression), "Takes the hiss and background noise out of your microphone before it is sent."));
        v.push((Elem::Slider { id: "volume".into(), label: "Voice Volume".into(), n: 21, at: (c.volume / 10).min(20) as usize, shown: format!("{}%", c.volume) }, "Everyone you hear."));
        let mut names = vec!["Default".to_string()];
        let mut at = 0;
        match &self.devices {
            Some(d) => {
                names.extend(d.iter().map(|x| x.1.clone()));
                at = d.iter().position(|x| x.0 == c.input).map_or(0, |k| k + 1);
            }
            None if !c.input.is_empty() => {
                names.push(c.input.clone());
                at = 1;
            }
            None => {}
        }
        v.push((Elem::Choice { id: "device".into(), label: "Microphone Device".into(), items: names, at }, "The microphone voice chat listens to."));
        v.push((Elem::Input { id: "channel".into(), label: "Channel".into(), text: c.channel.clone(), max: CHANNEL_MAX }, "Only players on the same channel hear each other. Leave it empty to talk with everyone."));
        v.push((Elem::Section { id: "people".into(), label: "Players on the Network".into(), open: true }, ""));
        if self.status.peers.is_empty() {
            v.push((Elem::Text { id: "nobody".into(), text: "No one found yet.".into(), dim: true, colour: None }, ""));
        } else {
            let rows = self
                .status
                .peers
                .iter()
                .map(|p| {
                    let note = if !p.same {
                        if p.channel.is_empty() { "no channel".to_string() } else { format!("channel {}", p.channel) }
                    } else if p.muted {
                        "muted".into()
                    } else if p.talking {
                        "talking".into()
                    } else {
                        String::new()
                    };
                    let colour = if p.muted || !p.same { Some([150, 150, 150, 255]) } else if p.talking { Some([120, 230, 120, 255]) } else { None };
                    Row { cells: vec![p.name.clone(), note, p.addr.ip().to_string()], icon: None, colour }
                })
                .collect();
            let columns = vec![Col { title: String::new(), width: 0.0, right: false }, Col { title: String::new(), width: 110.0, right: false }, Col { title: String::new(), width: 110.0, right: true }];
            v.push((Elem::List { id: "players".into(), rows, columns, at: 0, top: 0, height: self.status.peers.len().clamp(1, 6) }, "Enter mutes or unmutes the player."));
        }
        v.push((Elem::Input { id: "add".into(), label: "Add by Address".into(), text: String::new(), max: 64 }, "A computer's address or name on this network (192.168.1.20, or deck.local), for when it is not found by itself."));
        if !c.peers.is_empty() {
            let rows = c.peers.iter().map(|a| Row { cells: vec![a.clone(), if self.status.unresolved.contains(a) { "not on this network".into() } else { String::new() }], ..Default::default() }).collect();
            let columns = vec![Col { title: String::new(), width: 0.0, right: false }, Col { title: String::new(), width: 150.0, right: true }];
            v.push((Elem::List { id: "added".into(), rows, columns, at: 0, top: 0, height: c.peers.len().clamp(1, 4) }, "Enter removes the address."));
        }
        v.push((onoff("show", "Show Who Is Talking", ["On", "Off"], c.show_talking), "A list of the players talking, on the left of the screen."));
        v
    }

    /// The window's elements anew, its cursor kept on the same element, a line being typed and the lists' cursors kept.
    fn rebuild(&mut self) {
        let fresh = self.elems();
        let win = &mut self.win;
        let at_id = win.elems.get(win.at).and_then(|e| e.id()).map(str::to_string);
        let editing = win.edit.then(|| win.elems.get(win.at).cloned()).flatten();
        let mut elems = Vec::with_capacity(fresh.len());
        let mut meta = Vec::with_capacity(fresh.len());
        for (mut e, tip) in fresh {
            let old = e.id().and_then(|id| win.elems.iter().find(|o| o.id() == Some(id)));
            match (&mut e, old) {
                (Elem::List { at, top, rows, .. }, Some(Elem::List { at: oa, top: ot, .. })) => {
                    *at = (*oa).min(rows.len().saturating_sub(1));
                    *top = *ot;
                }
                (Elem::Section { open, .. }, Some(Elem::Section { open: oo, .. })) => *open = *oo,
                _ => {}
            }
            if let Some(ed) = &editing
                && ed.id().is_some()
                && ed.id() == e.id()
                && matches!(ed, Elem::Input { .. } | Elem::Slider { .. })
            {
                e = ed.clone();
            }
            elems.push(e);
            meta.push(Meta { tip: tip.into(), page: None });
        }
        let at = at_id.and_then(|id| elems.iter().position(|e| e.id() == Some(id.as_str())));
        (win.elems, win.meta) = (elems, meta);
        match at {
            Some(i) => win.at = i,
            None => win.edit = false,
        }
        win.settle();
        // The talking list: you first, then the others on this channel.
        let mut lines = Vec::new();
        if self.status.talking {
            lines.push("You".to_string());
        }
        lines.extend(self.status.peers.iter().filter(|p| p.talking && p.same && !p.muted).map(|p| p.name.clone()));
        self.talk.elems = lines.into_iter().map(|t| Elem::Text { id: String::new(), text: format!("\u{25b6} {t}"), dim: false, colour: Some([140, 235, 140, 255]) }).collect();
        self.talk.meta = vec![Meta::default(); self.talk.elems.len()];
        self.talk.open = self.engine.is_some() && self.cfg.show_talking && !self.talk.elems.is_empty();
    }

    fn changed(&mut self) {
        self.dirty = true;
        self.failed = None;
        if let Some(e) = &self.engine {
            e.apply(&self.cfg);
        }
    }

    fn events(&mut self, ev: Vec<Event>) {
        for e in ev {
            match e {
                Event::Set { id, index } => {
                    let c = &mut self.cfg;
                    match id.as_str() {
                        "on" => c.enabled = index == 0,
                        "mic" => c.mic_muted = index == 1,
                        "noise" => c.noise_suppression = index == 0,
                        "sound" => c.deafened = index == 1,
                        "talk" => c.talk = if index == 0 { Talk::Voice } else { Talk::Push },
                        "show" => c.show_talking = index == 0,
                        "ptt" => c.ptt_key = PTT_KEYS[index.min(PTT_KEYS.len() - 1)].0.into(),
                        "threshold" => c.threshold_db = DB_LOW + DB_STEP * index as i32,
                        "gain" => c.mic_gain = index as u32 * 10,
                        "volume" => c.volume = index as u32 * 10,
                        "device" => {
                            c.input = if index == 0 { String::new() } else { self.devices.as_ref().and_then(|d| d.get(index - 1)).map_or(c.input.clone(), |d| d.0.clone()) };
                        }
                        _ => continue,
                    }
                    self.changed();
                }
                Event::Text { id, text, done: true } => {
                    let t = text.trim().to_string();
                    match id.as_str() {
                        "channel" if t != self.cfg.channel => self.cfg.channel = voice::wire::clip(&t, CHANNEL_MAX),
                        "add" if !t.is_empty() && !self.cfg.peers.contains(&t) => self.cfg.peers.push(t),
                        _ => continue,
                    }
                    self.changed();
                }
                Event::Select { id, index } => {
                    match id.as_str() {
                        "players" => {
                            let Some(p) = self.status.peers.get(index) else { continue };
                            let name = p.name.to_lowercase();
                            if let Some(k) = self.cfg.muted.iter().position(|m| *m == name) {
                                self.cfg.muted.remove(k);
                            } else {
                                self.cfg.muted.push(name);
                            }
                        }
                        "added" if index < self.cfg.peers.len() => {
                            self.cfg.peers.remove(index);
                        }
                        _ => continue,
                    }
                    self.changed();
                }
                _ => {}
            }
        }
    }
}

/// Ctrl+Alt+Shift+V: the window opens (or closes).
pub fn toggle(w: &mut World) {
    let v = &mut w.voice;
    v.win.open = !v.win.open;
    v.win.focus = v.win.open;
    if v.win.open {
        let _ = v.win.end_edit();
        // Off every stop: settle puts the cursor on the first, on its marked option.
        v.win.at = usize::MAX;
        // Listing the devices can take a moment.
        if v.devices.is_none() && v.listing.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let _ = std::thread::Builder::new().name("voice-devices".into()).spawn(move || {
                let _ = tx.send(voice::devices());
            });
            v.listing = Some(rx);
        }
        v.rebuild();
    }
    w.sounds.push(if v.win.open { Se::Open } else { Se::Cancel } as u32);
}

/// Every frame: the engine started or stopped as the settings say, the windows brought up to date.
pub fn tick(w: &mut World) {
    let name = w.name.clone();
    let v = &mut w.voice;
    if let Some(rx) = &v.listing
        && let Ok(d) = rx.try_recv()
    {
        (v.devices, v.listing) = (Some(d), None);
    }
    match (&v.engine, v.cfg.enabled) {
        (None, true) if v.failed.is_none() && !name.is_empty() => match voice::Engine::start(&v.cfg, &name, &v.output) {
            Ok(e) => v.engine = Some(e),
            Err(e) => v.failed = Some(format!("{e:#}")),
        },
        (Some(_), false) => v.engine = None,
        _ => {}
    }
    match &mut v.engine {
        Some(e) => {
            e.tick();
            e.set_name(&name);
            v.status = e.status();
        }
        None => v.status = Default::default(),
    }
    if v.win.open || v.cfg.show_talking {
        v.rebuild();
    }
}

pub fn key(w: &mut World, key: egui::Key) {
    let (ev, se) = w.voice.win.key(key);
    w.voice.events(ev);
    w.sounds.extend(se.into_iter().map(|s| s as u32));
}

pub fn text(w: &mut World, events: &[egui::Event]) {
    let (ev, se) = w.voice.win.text(events);
    w.voice.events(ev);
    w.sounds.extend(se.into_iter().map(|s| s as u32));
}

/// The open windows (0 the settings, 1 who is talking), for the modern UI's panels.
pub fn windows(w: &World) -> Vec<(usize, Window)> {
    [&w.voice.win, &w.voice.talk].into_iter().enumerate().filter(|(_, x)| x.open).map(|(k, x)| (k, x.clone())).collect()
}

/// A click on a modern panel.
pub fn click(w: &mut World, which: usize, c: super::modern::ModClick) {
    if which != 0 {
        return;
    }
    w.voice.win.focus = true;
    let (ev, se) = c.apply(&mut w.voice.win);
    w.voice.events(ev);
    w.sounds.extend(se.into_iter().map(|s| s as u32));
}

/// The classic places: the settings centred, the talking list on the left.
fn classic_places(v: &VoiceChat, sw: f32, sh: f32) -> [Option<(f32, f32)>; 2] {
    let (ww, wh) = v.win.classic_size();
    [v.win.open.then(|| (((sw - ww) / 2.0).max(0.0).round(), ((sh - wh) / 2.0).max(0.0).round())), v.talk.open.then(|| (16.0, (sh * 0.42).round()))]
}

/// The classic UI's windows; their rects.
pub fn draw_classic(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let places = classic_places(&w.voice, sw, sh);
    w.voice.places.set(places);
    let mut out = Vec::new();
    if let Some((x, y)) = places[1] {
        w.voice.talk.draw_classic(gu, None, x, y);
    }
    if let Some((x, y)) = places[0] {
        out.push(w.voice.win.draw_classic(gu, None, x, y));
    }
    out
}

/// The pointer on the classic settings window; whether it is over a control.
pub fn pointer_classic(w: &mut World, gu: &GameUi, p: (f32, f32), moved: bool, clicked: bool, held: (bool, bool, bool)) -> bool {
    let Some((x, y)) = w.voice.places.get()[0].filter(|_| w.voice.win.open) else { return false };
    let (ww, wh) = w.voice.win.classic_size();
    if w.voice.win.drag.is_none() && w.voice.win.press.is_none() && (p.0 < x || p.1 < y || p.0 >= x + ww || p.1 >= y + wh) {
        return false;
    }
    let (hover, ev, se) = w.voice.win.pointer_classic(gu, x, y, p, moved, clicked, held);
    w.voice.events(ev);
    w.sounds.extend(se.into_iter().map(|s| s as u32));
    hover
}
