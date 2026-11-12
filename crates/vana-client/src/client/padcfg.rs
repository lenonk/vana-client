//! Ours: the Controller window's settings (the window is modern/padwin.rs). Retail's Config > Gamepad starts
//! FFXiPadConfig.exe (FUN_101bccb0), which saves padsin000 / padexsin000 / padmode000 in the registry (padreg.rs); this
//! is that tool's XInput page (dialog 147) inside the client: each function's button in padsin's 32 positions, the
//! "Button Combination Settings" (padexsin, used while Button Combination is held), and the tool's options. Its
//! labels and help are the tool's (US string table 102..139, dialog 147). The window writes config.json
//! (gamepad_padsin / gamepad_padexsin); the game reads them through client/gamepad.rs.

use super::gamepad::{self, Capture, Source};

/// What a row sets: one padsin position, or a direction of a directional control device (Movement, Camera, Menus),
/// which also takes a stick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Button,
    /// The sign of its direction in the stick values (up / left -1, down / right +1) and its pair's position.
    Dir(i8, usize),
}

/// A row: the label, the padsin position, its kind, the tool's help.
pub struct Row {
    pub label: &'static str,
    pub pos: usize,
    pub kind: Kind,
    pub help: &'static str,
}

const fn button(label: &'static str, pos: usize, help: &'static str) -> Row {
    Row { label, pos, kind: Kind::Button, help }
}

const fn dir(label: &'static str, pos: usize, sign: i8, pair: usize) -> Row {
    Row { label, pos, kind: Kind::Dir(sign, pair), help: DIR_HELP }
}

const DIR_HELP: &str =
    "Press a button for this direction, or push a stick this way to give the device the whole stick.";

/// The groups and their rows, in the tool's order. Positions from padsin's permutation (gamepad::PERM, MODLOG
/// "Gamepad"): 7 slot 0 A, 5 slot 3 B, ..., 13..16 movement, 17..20 camera, 21..24 the d-pad.
pub const GROUPS: [(&str, &[Row]); 4] = [
    (
        "Buttons",
        &[
            button(
                "Select / Confirm",
                7,
                "Select / Confirm; specify the nearest target / speak to an NPC; open the action command window.",
            ),
            button("Cancel", 5, "Cancel."),
            button("Active Window / Window Options", 8, "Select the active window; open the Options sub-windows, etc."),
            button("Main Menu", 6, "Open the Main Menu."),
            button("Autorun", 0, "Toggle autorun (while moving)."),
            button(
                "Macro Palette (Ctrl)",
                1,
                "Open macro palette #1 (Ctrl). In windows, held: page up and down; with the Alt button: change bags.",
            ),
            button(
                "Macro Palette (Alt)",
                3,
                "Open macro palette #2 (Alt). In windows, held: page up and down; with the Ctrl button: change bags.",
            ),
            button(
                "Logout",
                12,
                "Temporarily exit the game and return to PlayOnline (not a true log out: your character stays in the game world).",
            ),
            button("Hide Menus", 9, "Hide all windows (press once more to show them again)."),
            button("Shutdown", 27, "The shut down prompt."),
            button("Heal / Lock Target", 4, "Heal (with no target); lock on (with a target)."),
            button("Change View", 2, "Toggle the camera view."),
            button(
                "Take Screenshot",
                25,
                "Take a screenshot (only while the menus are hidden). With no window open: the chat line.",
            ),
            button("Target Nearby NPC", 28, "Target the nearest NPC."),
            button(
                "Operate menus with movement controls",
                10,
                "Hold down to operate menu options with the movement controls.",
            ),
            button(
                "Operate camera with movement controls",
                11,
                "Hold down to operate the camera with the movement controls.",
            ),
            button(
                "Toggle movement, menu, and/or camera controls",
                26,
                "Each press toggles the directional control device between movement, camera and/or menu controls.",
            ),
            button("Button Combination", 29, "Held: the other buttons do what Button Combination Settings give them."),
            button("Jump (/jump)", 30, "/jump."),
            button("Map (/map)", 31, "/map."),
        ],
    ),
    ("Movement", &[dir("Up", 13, -1, 14), dir("Down", 14, 1, 13), dir("Left", 15, -1, 16), dir("Right", 16, 1, 15)]),
    ("Camera", &[dir("Up", 17, -1, 18), dir("Down", 18, 1, 17), dir("Left", 19, -1, 20), dir("Right", 20, 1, 19)]),
    ("Menus", &[dir("Up", 21, -1, 22), dir("Down", 22, 1, 21), dir("Left", 23, -1, 24), dir("Right", 24, 1, 23)]),
];

/// The rows in order, with their group's name.
pub fn rows() -> impl Iterator<Item = (&'static str, &'static Row)> {
    GROUPS.iter().flat_map(|(g, rows)| rows.iter().map(move |r| (*g, r)))
}

pub fn row(i: usize) -> Option<(&'static str, &'static Row)> {
    rows().nth(i)
}

pub fn row_count() -> usize {
    GROUPS.iter().map(|g| g.1.len()).sum()
}

/// padsin's button numbers (XInput's virtual ones, table 0x10329810) by name.
pub const BUTTONS: [&str; 16] = [
    "B",
    "X",
    "Y",
    "A",
    "D-pad right",
    "D-pad left",
    "D-pad up",
    "D-pad down",
    "LB",
    "LT",
    "L3",
    "RB",
    "RT",
    "R3",
    "Start",
    "Back",
];

/// A padsin value as shown: unassigned, a button, or a stick axis (32 + code; negative inverted).
pub fn name(v: i32) -> String {
    match v {
        -1 => "Unassigned".into(),
        0..=15 => BUTTONS[v as usize].into(),
        _ if v.abs() >= 32 => {
            let axis = match v.abs() - 31 {
                1 => "Left stick X",
                2 => "Left stick Y",
                3 => "Z axis",
                4 => "Right stick X",
                5 => "Right stick Y",
                6 => "Rz axis",
                7 | 8 => "Slider",
                _ => "Hat switch",
            };
            if v < 0 { format!("{axis} (inverted)") } else { axis.into() }
        }
        _ => format!("Button {v}"),
    }
}

/// A padsin string's 32 values (missing ones -1), as FUN_10005130 reads it.
pub fn parse(s: &str) -> Vec<i32> {
    let mut v: Vec<i32> =
        s.split([',', ' ']).filter(|t| !t.is_empty()).map(|t| t.trim().parse().unwrap_or(-1)).collect();
    v.resize(32, -1);
    v
}

pub fn join(v: &[i32]) -> String {
    v.iter().map(i32::to_string).collect::<Vec<_>>().join(",")
}

/// Gives row `r` of `map` what the pad did: the first button pressed, else a stick pushed past `PUSH` (a direction
/// row only: the device's pair of positions take the axis, signed so that pushing this way goes this way). The
/// button or axis leaves any other position it held, as one button does one thing (FUN_10005130 keeps the last).
/// False when the pad did nothing usable (`sticks`: the map reads axes; the combination map does not).
pub fn assign(map: &mut [i32], r: &Row, cap: &Capture, sticks: bool) -> bool {
    if let Some(&b) = cap.pressed.first() {
        let b = b as i32;
        for v in map.iter_mut() {
            if *v == b {
                *v = -1;
            }
        }
        // A direction that had its device's stick: the pair lets go of it too (read() takes the axis from either).
        if let Kind::Dir(_, pair) = r.kind
            && map[r.pos].abs() >= 32
            && map[pair] == map[r.pos]
        {
            map[pair] = -1;
        }
        map[r.pos] = b;
        return true;
    }
    let Kind::Dir(sign, pair) = r.kind else { return false };
    if !sticks {
        return false;
    }
    let Some((k, a)) = cap.axes.iter().copied().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())) else {
        return false;
    };
    if a.abs() < PUSH {
        return false;
    }
    let code = [1, 2, 4, 5][k];
    // read(): the destination gets axis * sign(value); pushed this way (a's sign) it must come out as `sign`.
    let v = (31 + code) * if (a > 0.0) == (sign > 0) { 1 } else { -1 };
    for x in map.iter_mut() {
        if x.abs() == 31 + code {
            *x = -1;
        }
    }
    (map[r.pos], map[pair]) = (v, v);
    true
}

/// How far a stick must go to be taken (of 127).
pub const PUSH: f32 = 90.0;

/// The tool's Copy Assignment: the combination map's unassigned positions take the button settings' buttons; a button
/// already used there leaves its old position ("duplicate settings will be deleted"). Button Combination itself is
/// left out.
pub fn copy_assignment(main: &[i32], alt: &mut [i32]) {
    for p in 0..main.len().min(alt.len()) {
        let b = main[p];
        if p == 29 || alt[p] != -1 || !(0..32).contains(&b) {
            continue;
        }
        for x in alt.iter_mut() {
            if *x == b {
                *x = -1;
            }
        }
        alt[p] = b;
    }
}

/// The window's state, on World. The app fills `set` and `pads` each frame from config.json and the gamepad, and
/// takes `save` / `retail` / `reread` back (client/mod.rs).
#[derive(Default)]
pub struct PadCfg {
    pub open: bool,
    /// The settings in use (gamepad::settings).
    pub set: Option<gamepad::Settings>,
    /// The stick dead zone in % (config gamepad_deadzone).
    pub dead: u32,
    /// Retail's registry values' source, when found.
    pub retail: Option<String>,
    /// The connected pads, the one that plays first.
    pub pads: Vec<String>,
    /// 0 Button Settings (padsin), 1 Button Combination Settings (padexsin).
    pub tab: usize,
    /// The row the cursor is on (rows()), or one of the option rows after them (OPTS).
    pub row: usize,
    /// Waiting for the pad on the cursor's row: seconds left.
    pub listening: Option<f32>,
    /// To store: the maps, on, background, dead zone.
    pub save: Option<(String, String, bool, bool, u32)>,
    /// To store: the rumble option (config gamepad_rumble).
    pub save_rumble: Option<bool>,
    /// Clear config.json's maps (retail's or the default then play).
    pub use_retail: bool,
    /// Read the registry again.
    pub reread: bool,
    /// What the last assignment did (shown under the list).
    pub note: String,
    /// The list scrolls the cursor's row into view on the next frame (it moved by key, or the window opened).
    pub follow: std::cell::Cell<bool>,
    /// Bring the window in front on the next frame (it was just opened, over the Settings window it is opened from).
    pub raise: bool,
}

/// How long the window waits for a button.
pub const LISTEN: f32 = 6.0;

impl PadCfg {
    /// The maps shown: padsin and padexsin.
    pub fn maps(&self) -> [Vec<i32>; 2] {
        match &self.set {
            Some(s) => [parse(&s.padsin), parse(&s.padexsin)],
            None => [parse(gamepad::DEFAULT_PADSIN), parse(gamepad::DEFAULT_PADEXSIN)],
        }
    }

    fn store(&mut self, maps: [Vec<i32>; 2]) {
        let (on, bg) = self.set.as_ref().map_or((true, false), |s| (s.on, s.background));
        self.save = Some((join(&maps[0]), join(&maps[1]), on, bg, self.dead));
        if let Some(s) = self.set.as_mut() {
            (s.padsin, s.padexsin, s.source) = (join(&maps[0]), join(&maps[1]), Source::Config);
        }
    }

    /// Stores the options (the maps as they are).
    pub fn set_options(&mut self, on: bool, background: bool, dead: u32) {
        if let Some(s) = self.set.as_mut() {
            (s.on, s.background) = (on, background);
        }
        self.dead = dead;
        self.store(self.maps());
    }

    /// Stores the rumble option. Retail's setting (padmode000) stops deciding it once config.json has maps, so
    /// the maps are stored with it.
    pub fn set_rumble(&mut self, on: bool) {
        if let Some(s) = self.set.as_mut() {
            s.rumble = on;
        }
        self.save_rumble = Some(on);
        self.store(self.maps());
    }

    /// The pad's input while listening: assigns the cursor's row, or keeps waiting.
    pub fn feed(&mut self, cap: &Capture, dt: f32) -> bool {
        let Some(left) = self.listening else { return false };
        let Some((_, r)) = row(self.row) else {
            self.listening = None;
            return false;
        };
        let mut maps = self.maps();
        let tab = self.tab.min(1);
        if assign(&mut maps[tab], r, cap, tab == 0) {
            self.note = format!("{}: {}.", r.label, name(maps[tab][r.pos]));
            self.listening = None;
            self.store(maps);
            return true;
        }
        let left = left - dt;
        self.listening = (left > 0.0).then_some(left);
        if self.listening.is_none() {
            self.note = "Nothing pressed: left as it was.".into();
        }
        false
    }

    /// The cursor's row unassigned.
    pub fn clear(&mut self) {
        let Some((_, r)) = row(self.row) else { return };
        let mut maps = self.maps();
        let tab = self.tab.min(1);
        maps[tab][r.pos] = -1;
        if let Kind::Dir(_, pair) = r.kind
            && maps[tab][pair].abs() >= 32
        {
            maps[tab][pair] = -1;
        }
        self.note = format!("{}: unassigned.", r.label);
        self.store(maps);
    }

    /// FFXiPadConfig's XInput setup (its "XInput" predefined setup, the default).
    pub fn defaults(&mut self) {
        self.note = "The XInput setup.".into();
        self.store([parse(gamepad::DEFAULT_PADSIN), parse(gamepad::DEFAULT_PADEXSIN)]);
    }

    pub fn copy(&mut self) {
        let [main, mut alt] = self.maps();
        copy_assignment(&main, &mut alt);
        self.note = "Copied the button settings to the unassigned combination settings.".into();
        self.store([main, alt]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every position but the stick-only ones has a row, once; the default map reads as the tool's XInput layout.
    #[test]
    fn rows_cover_padsin() {
        let mut seen = [0; 32];
        for (_, r) in rows() {
            seen[r.pos] += 1;
        }
        assert!(seen.iter().all(|&n| n == 1), "{seen:?}");
        let m = parse(gamepad::DEFAULT_PADSIN);
        let of = |label: &str| name(m[rows().find(|r| r.1.label == label).unwrap().1.pos]);
        assert_eq!(of("Select / Confirm"), "A");
        assert_eq!(of("Cancel"), "B");
        assert_eq!(of("Main Menu"), "X");
        assert_eq!(of("Active Window / Window Options"), "Y");
        assert_eq!(of("Autorun"), "LB");
        assert_eq!(of("Take Screenshot"), "RB");
        assert_eq!(of("Logout"), "Start");
        assert_eq!(of("Hide Menus"), "Back");
        assert_eq!(m[13], -33);
        assert_eq!(join(&m), gamepad::DEFAULT_PADSIN);
    }

    /// A button moves to the row it is given; a stick pushed up on Movement Up gives forward and back the left stick's
    /// Y axis inverted (up is positive in XInput, forward negative), as the default.
    #[test]
    fn assigning() {
        let mut m = parse(gamepad::DEFAULT_PADSIN);
        let r = |label: &str, group: &str| rows().find(|r| r.0 == group && r.1.label == label).unwrap().1;
        let cap = |pressed: Vec<u8>, axes: [f32; 4]| Capture { pressed, axes };
        // A (3) on Cancel: Select / Confirm loses it.
        assert!(assign(&mut m, r("Cancel", "Buttons"), &cap(vec![3], [0.0; 4]), true));
        assert_eq!((m[5], m[7]), (3, -1));
        // A button on Movement Up: Down lets go of the left stick too (it would still move forward).
        assert!(assign(&mut m, r("Up", "Movement"), &cap(vec![8], [0.0; 4]), true));
        assert_eq!((m[13], m[14]), (8, -1));
        // A stick on a button row does nothing.
        assert!(!assign(&mut m, r("Cancel", "Buttons"), &cap(vec![], [127.0, 0.0, 0.0, 0.0]), true));
        let mut m = vec![-1; 32];
        assert!(assign(&mut m, r("Up", "Movement"), &cap(vec![], [10.0, 127.0, 0.0, 0.0]), true));
        assert_eq!((m[13], m[14]), (-33, -33));
        // Right stick pushed right on Camera Right: lRx as is; pushed left on Camera Right: inverted.
        assert!(assign(&mut m, r("Right", "Camera"), &cap(vec![], [0.0, 0.0, 120.0, 0.0]), true));
        assert_eq!((m[19], m[20]), (35, 35));
        assert!(assign(&mut m, r("Right", "Camera"), &cap(vec![], [0.0, 0.0, -120.0, 0.0]), true));
        assert_eq!((m[19], m[20]), (-35, -35));
        // Under the push, and sticks in the combination map: nothing.
        assert!(!assign(&mut m, r("Up", "Menus"), &cap(vec![], [0.0, 50.0, 0.0, 0.0]), true));
        assert!(!assign(&mut m, r("Up", "Menus"), &cap(vec![], [0.0, 127.0, 0.0, 0.0]), false));
        // The pushes come out the right way through gamepad::read: forward (move Y -127) for the stick up.
        let s = join(&{
            let mut d = parse(gamepad::DEFAULT_PADSIN);
            assign(&mut d, r("Up", "Movement"), &cap(vec![], [0.0, -127.0, 0.0, 0.0]), true);
            d
        });
        let map = gamepad::PadMap::parse(&s);
        let up = |c: u8| if c == 2 { -127.0 } else { 0.0 };
        let v = gamepad::read(&map, &map, &gamepad::Device { buttons: &[], axis: &up }, 0, (false, false), 0.33).1;
        assert_eq!(v[3], -127.0, "stick pulled down, given as Up: it moves forward");
    }

    #[test]
    fn copy_assignment_fills_gaps() {
        let main = parse(gamepad::DEFAULT_PADSIN);
        let mut alt = parse(gamepad::DEFAULT_PADEXSIN);
        alt[0] = 3; // A on Autorun in the combination map.
        copy_assignment(&main, &mut alt);
        // Autorun is taken (kept), then A goes to Select / Confirm (7) and leaves Autorun as a duplicate.
        assert_eq!((alt[7], alt[0], alt[5]), (3, -1, 0));
        assert_eq!(alt[29], -1);
    }

    /// config.json's combination map counts alone, whatever the main map's source.
    #[test]
    fn combination_override_alone() {
        let cfg = crate::config::Config { gamepad_padexsin: "3".into(), ..Default::default() };
        let r = crate::padreg::Retail { padsin: Some("8".into()), padexsin: Some("9".into()), ..Default::default() };
        let s = gamepad::settings(&cfg, Some(&r));
        assert_eq!((s.padsin.as_str(), s.padexsin.as_str(), s.source, s.saved), ("8", "3", Source::Retail, true));
        let s = gamepad::settings(&cfg, None);
        assert_eq!((s.padexsin.as_str(), s.source), ("3", Source::Default));
    }

    #[test]
    fn window_state() {
        let mut p = PadCfg { dead: 33, ..Default::default() };
        p.set = Some(gamepad::settings(&crate::config::Config::default(), None));
        p.row = rows().position(|r| r.1.label == "Main Menu").unwrap();
        p.listening = Some(LISTEN);
        assert!(!p.feed(&Capture::default(), 1.0));
        assert_eq!(p.listening, Some(LISTEN - 1.0));
        assert!(p.feed(&Capture { pressed: vec![2], axes: [0.0; 4] }, 0.1));
        let (padsin, _, on, bg, dead) = p.save.clone().unwrap();
        assert_eq!((parse(&padsin)[6], parse(&padsin)[8], on, bg, dead), (2, -1, true, false, 33));
        assert_eq!(p.set.as_ref().unwrap().source, Source::Config);
        p.listening = Some(0.5);
        assert!(!p.feed(&Capture::default(), 1.0));
        assert!(p.listening.is_none());
    }
}
