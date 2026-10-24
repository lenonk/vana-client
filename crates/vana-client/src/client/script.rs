//! Scripted input for test drives: synthetic egui events fed in before each frame, so the client's
//! own input handling runs without OS-level injection. Steps are separated by ';':
//! `wait:SECS`, `until:PHASE[,SECS]` (go on after SECS anyway; login, title, lobby, event, world; also `cam:TAG` while an event shot plays and
//! `msg:ID` while that event message is open, `noevent` while none runs), `type:TEXT`, `user`, `pass`,
//! `key:[shift+|ctrl+|alt+]NAME` (egui key names: Tab, Enter, Escape, Space, Minus, ArrowDown...),
//! `move:X,Y`, `click:X,Y` and `rclick:X,Y` (physical pixels), `shot:PATH`, and `press:KEY,SECS,PHASE` (press KEY every
//! SECS until the client reaches PHASE; event prompts), `every:KEY,SECS` (press KEY every SECS alongside the
//! steps that follow; `every:off` stops it), `act:NAME` for keys egui cannot name (numpad*), and `follow:PATH`: lines
//! appended to PATH later each run as a script of their own, until a line `end`.

use anyhow::{Result, anyhow, bail};
use std::path::PathBuf;
use std::time::Instant;

enum Step {
    Wait(f32),
    /// A phase or probe, and an optional limit in seconds after which the script goes on anyway.
    Until(String, Option<f32>),
    Text(String),
    Key(egui::Key, egui::Modifiers),
    Move(f32, f32),
    Button(bool),
    /// A right-button press and release.
    Right,
    Shot(PathBuf),
    Repeat { key: egui::Key, every: f32, until: String, last: Option<Instant> },
    Action(String),
    Every(Option<(egui::Key, f32)>),
    /// A file read as it grows, and how much of it was taken.
    Follow { path: PathBuf, read: usize },
}

pub struct Script {
    steps: Vec<Step>,
    at: usize,
    since: Instant,
    pointer: egui::Pos2,
    /// A key pressed in the background, its period and the last press.
    every: Option<(egui::Key, f32, Instant)>,
    /// The script types the login itself (`user` / `pass` steps); otherwise --credentials logs in.
    pub logs_in: bool,
}

impl Script {
    /// `creds` supplies the text for the `user` and `pass` steps.
    pub fn parse(spec: &str, creds: Option<&(String, String)>) -> Result<Self> {
        let mut steps = Vec::new();
        let mut logs_in = false;
        for raw in spec.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let (op, arg) = raw.split_once(':').unwrap_or((raw, ""));
            let xy = || -> Result<(f32, f32)> {
                let (x, y) = arg.split_once(',').ok_or_else(|| anyhow!("{raw}: want X,Y"))?;
                Ok((x.trim().parse()?, y.trim().parse()?))
            };
            match op {
                "wait" => steps.push(Step::Wait(arg.parse()?)),
                "until" => {
                    let (p, secs) = match arg.split_once(',') {
                        Some((p, s)) => (p, Some(s.parse()?)),
                        None => (arg, None),
                    };
                    steps.push(Step::Until(p.to_string(), secs));
                }
                "type" => steps.push(Step::Text(arg.to_string())),
                "user" | "pass" => {
                    let (u, p) = creds.ok_or_else(|| anyhow!("{op} needs --credentials"))?;
                    logs_in = true;
                    steps.push(Step::Text(if op == "user" { u.clone() } else { p.clone() }));
                }
                "key" => {
                    let (mut mods, mut name) = (egui::Modifiers::NONE, arg);
                    for (p, m) in [("shift+", egui::Modifiers::SHIFT), ("ctrl+", egui::Modifiers::CTRL), ("alt+", egui::Modifiers::ALT)] {
                        if let Some(n) = name.strip_prefix(p) {
                            (mods, name) = (mods | m, n);
                        }
                    }
                    let key = egui::Key::from_name(name).ok_or_else(|| anyhow!("unknown key {name}"))?;
                    steps.push(Step::Key(key, mods));
                }
                "move" => {
                    let (x, y) = xy()?;
                    steps.push(Step::Move(x, y));
                }
                "click" => {
                    let (x, y) = xy()?;
                    steps.extend([Step::Move(x, y), Step::Button(true), Step::Button(false)]);
                }
                "rclick" => {
                    let (x, y) = xy()?;
                    steps.extend([Step::Move(x, y), Step::Right]);
                }
                "shot" => steps.push(Step::Shot(PathBuf::from(arg))),
                "follow" => steps.push(Step::Follow { path: PathBuf::from(arg), read: 0 }),
                "act" => steps.push(Step::Action(arg.to_string())),
                "every" if arg == "off" => steps.push(Step::Every(None)),
                "every" => {
                    let (name, secs) = arg.split_once(',').ok_or_else(|| anyhow!("{raw}: want KEY,SECS"))?;
                    let key = egui::Key::from_name(name).ok_or_else(|| anyhow!("unknown key {name}"))?;
                    steps.push(Step::Every(Some((key, secs.parse()?))));
                }
                "press" => {
                    let f: Vec<&str> = arg.split(',').collect();
                    let [name, every, until] = f[..] else { bail!("{raw}: want KEY,SECS,PHASE") };
                    let key = egui::Key::from_name(name).ok_or_else(|| anyhow!("unknown key {name}"))?;
                    steps.push(Step::Repeat { key, every: every.parse()?, until: until.to_string(), last: None });
                }
                _ => bail!("unknown step {raw}"),
            }
        }
        Ok(Self { steps, at: 0, since: Instant::now(), pointer: egui::Pos2::ZERO, every: None, logs_in })
    }

    /// This frame's events (one step per frame), a screenshot to take and an action, given the current phase
    /// and what else `until:` can wait for (`cam:TAG`, `msg:ID`).
    pub fn next(&mut self, phase: &str, probes: &[String], pixels_per_point: f32) -> (Vec<egui::Event>, Option<PathBuf>, Option<String>) {
        let mut events = Vec::new();
        let mut shot = None;
        let mut action = None;
        if let Some((key, secs, last)) = &mut self.every
            && last.elapsed().as_secs_f32() >= *secs
        {
            *last = Instant::now();
            for pressed in [true, false] {
                events.push(egui::Event::Key { key: *key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE });
            }
        }
        if let Some(Step::Follow { path, read }) = self.steps.get_mut(self.at) {
            let text = std::fs::read_to_string(&*path).unwrap_or_default();
            let Some(cut) = text.get(*read..).and_then(|t| t.rfind('\n')) else { return (events, shot, action) };
            let lines: Vec<String> = text[*read..*read + cut].lines().map(str::to_string).collect();
            *read += cut + 1;
            let (mut insert, mut end) = (Vec::new(), false);
            for l in lines {
                if l.trim() == "end" {
                    end = true;
                    break;
                }
                match Script::parse(&l, None) {
                    Ok(s) => insert.extend(s.steps),
                    Err(e) => log::warn!("script: {e}"),
                }
            }
            if end {
                self.steps.remove(self.at);
            }
            let at = self.at;
            self.steps.splice(at..at, insert);
            self.since = Instant::now();
            return (events, shot, action);
        }
        let Some(step) = self.steps.get_mut(self.at) else { return (events, shot, action) };
        let done = match step {
            Step::Wait(s) => self.since.elapsed().as_secs_f32() >= *s,
            Step::Until(p, limit) => {
                let hit = phase == p || probes.iter().any(|x| x == p);
                let out = limit.is_some_and(|l| self.since.elapsed().as_secs_f32() >= l);
                if out && !hit {
                    log::info!("script: until {p} gave up after {:.0} s", limit.unwrap_or(0.0));
                }
                hit || out
            }
            Step::Text(t) => {
                events.push(egui::Event::Text(t.clone()));
                true
            }
            Step::Key(key, modifiers) => {
                for pressed in [true, false] {
                    events.push(egui::Event::Key { key: *key, physical_key: None, pressed, repeat: false, modifiers: *modifiers });
                }
                // winit sends the space and '/' as text too.
                if *key == egui::Key::Space {
                    events.push(egui::Event::Text(" ".into()));
                }
                if *key == egui::Key::Slash && modifiers.is_none() {
                    events.push(egui::Event::Text("/".into()));
                }
                true
            }
            Step::Action(a) => {
                action = Some(a.clone());
                true
            }
            Step::Every(e) => {
                self.every = e.map(|(k, s)| (k, s, Instant::now()));
                true
            }
            Step::Move(x, y) => {
                self.pointer = egui::pos2(*x / pixels_per_point, *y / pixels_per_point);
                events.push(egui::Event::PointerMoved(self.pointer));
                true
            }
            Step::Button(pressed) => {
                events.push(egui::Event::PointerButton { pos: self.pointer, button: egui::PointerButton::Primary, pressed: *pressed, modifiers: egui::Modifiers::NONE });
                true
            }
            Step::Right => {
                for pressed in [true, false] {
                    events.push(egui::Event::PointerButton { pos: self.pointer, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE });
                }
                true
            }
            Step::Shot(p) => {
                shot = Some(p.clone());
                true
            }
            Step::Follow { .. } => true,
            Step::Repeat { key, every, until, last } => {
                if phase == until || probes.iter().any(|x| x == until) {
                    true
                } else {
                    if last.is_none_or(|t| t.elapsed().as_secs_f32() >= *every) {
                        *last = Some(Instant::now());
                        for pressed in [true, false] {
                            events.push(egui::Event::Key { key: *key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE });
                        }
                    }
                    false
                }
            }
        };
        if done {
            self.at += 1;
            self.since = Instant::now();
        }
        (events, shot, action)
    }

    pub fn finished(&self) -> bool {
        self.at >= self.steps.len()
    }
}
