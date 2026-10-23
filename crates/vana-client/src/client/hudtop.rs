//! The always-open HUD windows: the top bar (titlewin + helpwind), the Network window (netstat) and the
//! compass with the Vana'diel time (DAT_10621980).

use super::World;
use super::game_ui::{GameUi, WHITE};
use glam::Vec3;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Instant;

#[derive(Default)]
pub struct HudTop {
    /// netstat: S / R bytes a second (stat[1] / stat[0]), the percentage, totals at the last 3 s tick.
    sent_rate: u64,
    recv_rate: u64,
    pct: i32,
    totals: (u64, u64),
    last_tick: Option<Instant>,
    /// Netmeter animation frame 0..3 and its tick counter.
    frame: usize,
    meter_ticks: f32,
    /// helpwind marquee (FUN_10217640): text, wait ticks, offset in pixels.
    help: String,
    help_wait: f32,
    help_off: f32,
    /// Compass open fraction k 0..1 (24 ticks, DAT_10379934).
    compass: f32,
    /// The clock's grid suffix (FUN_10220350): rebuilt when the countdown drops below 0, then 30 ticks on.
    grid: String,
    grid_wait: f32,
    last: Option<Instant>,
    /// The 0x8000 windows' fade (titlewin, helpwind, netstat).
    pub fade: TopFade,
    /// helpwind's busy strip (+0x64): keytops3 165 + frame, `busy_wait` ticks left on it, the last frame's state.
    busy_frame: usize,
    busy_wait: f32,
    busy_was: bool,
}

/// Text slots of helpwind (+0x20) and titlewin (+0xa4), FUN_10217230: index 0 action menu, 1 focused window,
/// 2 window-specific, 3 notice ticker. Empty = none.
#[derive(Default, Clone, Debug)]
pub struct BarText {
    pub help: [String; 4],
    pub title: [String; 4],
}

/// The shown slot (FUN_102178e0): 3 when set, else the highest non-empty of 2 > 1 > 0.
fn shown(slots: &[String; 4]) -> &str {
    slots.iter().rev().find(|s| !s.is_empty()).map_or("", String::as_str)
}

/// A window frame part's fade state (+0x44) and count (+0x48), FUN_10116230.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fade {
    Shown,
    Hidden,
    Out(i32),
    In(i32),
}

/// The top-bar rule (FUN_10159080, asm 0x1015951e..0x10159654) and the frame fade it drives, in 60 Hz ticks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TopFade {
    pub fade: Fade,
    /// Window manager +0x78.
    pub timer: i32,
    acc: f32,
}

impl Default for TopFade {
    /// Manager init (FUN_10157010): timer 30; parts start shown (FUN_10115fe0).
    fn default() -> Self {
        Self { fade: Fade::Shown, timer: 30, acc: 0.0 }
    }
}

impl TopFade {
    /// One tick. `rule` is false while zoning (game state 0x70), when the rule does not run.
    pub fn tick(&mut self, active: bool, rule: bool) {
        if rule {
            if !active {
                if self.timer > 0 {
                    self.timer -= 1;
                    if self.timer <= 0 {
                        self.fade = Fade::Out(20);
                        self.timer = 0;
                    }
                }
            } else if self.timer <= 0 {
                // Re-armed only once the bars are down: 180 empty ticks in total since they came up.
                self.fade = Fade::In(20);
                self.timer = 180;
            }
        }
        self.fade = match self.fade {
            Fade::Out(n) if n > 1 => Fade::Out(n - 1),
            Fade::Out(_) => Fade::Hidden,
            Fade::In(n) if n > 1 => Fade::In(n - 1),
            Fade::In(_) => Fade::Shown,
            f => f,
        };
    }

    /// Runs the whole ticks in `ticks` (fractions carry over).
    pub fn advance(&mut self, ticks: f32, active: bool, rule: bool) {
        self.acc += ticks;
        while self.acc >= 1.0 {
            self.acc -= 1.0;
            self.tick(active, rule);
        }
    }

    /// Alpha factor +0x0c.
    pub fn alpha(&self) -> f32 {
        match self.fade {
            Fade::Shown => 1.0,
            Fade::Hidden => 0.0,
            Fade::Out(n) => n as f32 * 0.05,
            Fade::In(n) => 1.0 - n as f32 * 0.05,
        }
    }
}

/// Zones with no compass, time only (FUN_100abd90): ships and airships.
fn no_compass(zone: u16) -> bool {
    matches!(zone, 1 | 3 | 0x2e | 0x2f | 0x3a..=0x3c | 0xdc | 0xdd | 0xdf..=0xe4)
}

impl HudTop {
    fn ticks(&mut self) -> f32 {
        let now = Instant::now();
        let t = self.last.map_or(0.0, |l| (now - l).as_secs_f32().min(0.5) * 60.0);
        self.last = Some(now);
        t
    }

    /// FUN_100f9ba0 / FUN_100f9b20, every 3000 ms: S / R = bytes * 1000 / 3000; the percentage gains 12 while
    /// the server has sent within 10 s, else loses 6.
    fn net_tick(&mut self, w: &World) {
        let now = Instant::now();
        let last = *self.last_tick.get_or_insert_with(|| {
            self.pct = 100;
            now
        });
        if now.duration_since(last).as_millis() < 3000 {
            return;
        }
        self.last_tick = Some(now);
        let s = &w.net.stats;
        let (sent, recv) = (s.sent.load(Relaxed), s.received.load(Relaxed));
        self.sent_rate = sent.saturating_sub(self.totals.0) * 1000 / 3000;
        self.recv_rate = recv.saturating_sub(self.totals.1) * 1000 / 3000;
        self.totals = (sent, recv);
        let quiet = s.now_ms().saturating_sub(s.last_rx_ms.load(Relaxed));
        self.pct = if quiet < 10_000 { (self.pct + 12).min(100) } else { (self.pct - 6).max(0) };
    }

    /// World entry / zone-in (FUN_1015e3f0): the bars up, the timer at 30.
    /// The cached grid position, "(X-N)" (FUN_101f7600(1)).
    pub fn grid(&self) -> &str {
        &self.grid
    }

    pub fn enter(&mut self) {
        self.fade = TopFade::default();
    }

    /// Draws the three windows from the bars' text slots.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, gu: &mut GameUi, w: &World, sw: f32, sh: f32, bars: &BarText, camera_forward: Vec3, log_top: f32) {
        let ticks = self.ticks();
        self.net_tick(w);
        let (title, help) = (shown(&bars.title), shown(&bars.help));
        // titlewin HN mode (+0x448) and titlehan are not modelled. A busy bar counts as active (FUN_10221740).
        // The rule runs in game state 0x60 only; zoning and the zone load are taken as 0x70 (inferred).
        let busy = w.scan.busy || w.scan.party_busy;
        self.fade.advance(ticks, !title.is_empty() || !help.is_empty() || busy, w.zoning_since.is_none() && w.loading.is_none());
        self.draw_bars(gu, sw, sh, title, help, ticks, busy);
        // The compass alone is hidden in events.
        self.draw_compass(gu, w, log_top, ticks, !super::event::active(w), camera_forward);
    }

    /// titlewin, helpwind and netstat at the current fade (also the offscreen topbar-shot).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_bars(&mut self, gu: &mut GameUi, sw: f32, sh: f32, title: &str, help: &str, ticks: f32, busy: bool) {
        let a = self.fade.alpha();
        // titlewin (16, 16) 112 x 30: the title at (+4, +8), clipped to width - 8 (FUN_1014a3b0); content always.
        if let Some((x, y)) = gu.window_alpha("menu    titlewin", sw, sh, a)
            && !title.is_empty()
        {
            gu.clip(Some([x + 4.0, y, 104.0, 30.0]));
            gu.text(title, x + 4.0, y + 8.0, 0, WHITE);
            gu.clip(None);
        }
        // helpwind (130, 16) stretched to W - 260 (FUN_10135360); "Help" caption 361 at (x1 - 0x50, y0) only
        // while fully shown (FUN_10117b60).
        let (hx, hy, hw) = (130.0, 16.0, sw - 260.0);
        if a > 0.0 {
            gu.frame_alpha(hx, hy, hw, 30.0, a);
        }
        if self.fade.fade == Fade::Shown {
            gu.sprite("menu    windowps", 361, hx + hw - 80.0, hy, 0x8080_8080);
        }
        // FUN_102214d0 / FUN_102213e0: while a request is pending (helpwind +0x64), keytops3 165..169 (8 ticks each,
        // table 0x10385fcc; a new request restarts at 165) centred in the text box at y0 + 8, unfaded, in place of
        // the text; the marquee stands still.
        if busy {
            if !self.busy_was {
                (self.busy_frame, self.busy_wait) = (0, 8.0);
            } else {
                self.busy_wait -= ticks;
                while self.busy_wait < 1.0 {
                    self.busy_frame = (self.busy_frame + 1) % 5;
                    self.busy_wait += 8.0;
                }
            }
            let half = (165..170).filter_map(|f| gu.cell_bounds("menu    keytops3", f, 0)).map(|b| (b[2] - b[0]) / 2.0).fold(0.0, f32::max);
            let x0 = gu.cell_bounds("menu    keytops3", 165 + self.busy_frame, 0).map_or(0.0, |b| b[0]);
            gu.sprite("menu    keytops3", 165 + self.busy_frame, sw / 2.0 - half - x0, hy + 8.0, 0x8080_8080);
        }
        self.busy_was = busy;
        let text = if busy { self.help.clone() } else { help.to_string() };
        if text != self.help {
            self.help = text;
            self.help_wait = 60.0;
            self.help_off = 0.0;
        }
        if !self.help.is_empty() && !busy {
            // Box (x0 + 6, y0 + 8), width - 12 (FUN_101354f0); wider text waits 60 ticks, then 1 px a tick.
            let (bx, bw) = (hx + 6.0, hw - 12.0);
            gu.clip(Some([bx, hy, bw, 30.0]));
            let width = gu.text_width(&self.help);
            if width <= bw {
                gu.text(&self.help, bx, hy + 8.0, 0, WHITE);
            } else {
                let looped = format!("{}  ", self.help);
                let cycle = gu.text_width(&looped);
                if self.help_wait > 0.0 {
                    self.help_wait -= ticks;
                } else {
                    self.help_off = (self.help_off + ticks) % cycle.max(1.0);
                }
                gu.text(&looped, bx - self.help_off, hy + 8.0, 0, WHITE);
                gu.text(&looped, bx - self.help_off + cycle, hy + 8.0, 0, WHITE);
            }
            gu.clip(None);
        }
        // netstat (W - 128, 16): mode 1 (network), FUN_101f8370. The frame and its "Network" sprite fade; the
        // content is drawn whatever the fade.
        if let Some((x, y)) = gu.window_alpha("menu    netstat", sw, sh, a) {
            let pct = self.pct;
            // Netmeter sprites in order 12, 0..11 (table 0x10378d0c); base 0 / 9 / 5 / 1; a frame every
            // (11 - pct / 10) draws, taken at 30 draws a second (inferred).
            let base = if pct == 0 { 0 } else if pct < 30 { 9 } else if pct < 60 { 5 } else { 1 };
            self.meter_ticks += ticks;
            let every = (11 - pct / 10).max(1) as f32 * 2.0;
            while self.meter_ticks >= every {
                self.meter_ticks -= every;
                self.frame = (self.frame + 1) % 4;
            }
            let order = [12, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
            let k = if base == 0 { order[0] } else { order[(base + self.frame).min(12)] };
            gu.sprite("menu    netmeter", k, x, y, 0x8080_8080);
            let end = gu.shape_text(&format!("{pct:3}"), x + 20.0, y + 10.0, 0x6080_8080, false);
            gu.shape_text("%", end, y + 10.0, 0x6080_8080, false);
            // Group 0xc 0x5c "S" / 0x5d "R".
            gu.shape_text("S", x + 58.0, y + 4.0, 0x6080_8080, false);
            gu.shape_text(&self.sent_rate.to_string(), x + 68.0, y + 4.0, 0x6080_8080, false);
            gu.shape_text("R", x + 58.0, y + 16.0, 0x6080_8080, false);
            gu.shape_text(&self.recv_rate.to_string(), x + 68.0, y + 16.0, 0x6080_8080, false);
        }
    }

    /// Fixed netstat numbers for offscreen shots.
    pub fn set_net(&mut self, pct: i32, sent: u64, recv: u64) {
        (self.pct, self.sent_rate, self.recv_rate) = (pct, sent, recv);
    }

    /// FUN_10216610 / FUN_10216450: the disk, turned so the camera's heading is at the top and tilted
    /// -1.0996 rad about X (scale 0.32 open), N E S W at radius 140, the time at (cx - 50, cy + 26).
    fn draw_compass(&mut self, gu: &mut GameUi, w: &World, log_top: f32, ticks: f32, show: bool, fwd: Vec3) {
        let step = ticks / 24.0;
        self.compass = if show { (self.compass + step).min(1.0) } else { (self.compass - step).max(0.0) };
        let k = self.compass;
        if k <= 0.0 {
            // Opening clears the suffix and its countdown, so the first draw builds it.
            self.grid.clear();
            self.grid_wait = 0.0;
            return;
        }
        // Centre (0x58, log top - 0x2a) (FUN_10215170 / object +0x28 .. +0x2c).
        let (cx, cy) = (88.0, log_top - 42.0);
        let alpha = (k * 128.0) as u32;
        let tint = alpha << 24 | 0x80_8080;
        let s = 0.8 * k * 0.4;
        let tilt = (1.0996f32).cos();
        // Screen up = the camera's horizontal forward; right = forward x -Y (east when facing north).
        let f = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or(Vec3::Z);
        let r = f.cross(Vec3::NEG_Y);
        let to_screen = |d: Vec3, radius: f32| -> (f32, f32) { (d.dot(r) * radius * s, -d.dot(f) * radius * s * tilt) };
        if !w.zone.is_some_and(no_compass) {
            // The disk: sprite 0's cells mapped through the same turn and tilt (north = texture up, inferred).
            let ang = Vec3::Z.dot(r).atan2(Vec3::Z.dot(f));
            let (sn, cs) = ang.sin_cos();
            gu.sprite_affine("menu    compass", 0, cx, cy, [cs * s, -sn * s, sn * s * tilt, cs * s * tilt], tint);
            for (dir, sprite) in [(Vec3::Z, 4), (Vec3::X, 1), (Vec3::NEG_Z, 3), (Vec3::NEG_X, 2)] {
                let (dx, dy) = to_screen(dir, 140.0);
                gu.sprite("menu    compass", sprite, cx + dx, cy + dy, tint);
            }
            // FUN_102203b0: the kaipage arrow on the rim toward the tracked target (colour 0x802080f0, the name in
            // white) or, untracked, the call-for-help monster (FUN_1021f030; 0x80802020, the name in ncol 8): radius
            // 28.8 x 0.8, 36 x 0.8 with NIN main or sub, through the 2k matrix, its bottom centre on the point, the
            // name at (+36, +26); both at a fixed alpha, not the disk's.
            let aim = match w.scan.tracking() {
                Some(t) => Some((t.pos[0], t.pos[2], t.index, 0x8020_80f0u32, 0x8080_8080u32)),
                None => super::target::call_for_help(w).and_then(|id| w.entities.get(&id)).map(|e| (e.pos.x, e.pos.z, e.index, 0x8080_2020, gu.ncol(8))),
            };
            if let Some((tx, tz, index, colour, name_colour)) = aim {
                let d = Vec3::new(tx - w.pos.x, 0.0, tz - w.pos.z).normalize_or_zero();
                if d != Vec3::ZERO {
                    let nin = w.stats.as_ref().is_some_and(|s| s.main.0 == 13 || s.sub.0 == 13);
                    let radius = if nin { 36.0 } else { 28.8 } * 0.8 * 2.0 * k;
                    let (sx, sy) = (d.dot(r) * radius, -d.dot(f) * radius * tilt);
                    let n = gu.lib_len("anc     kaipage").max(1);
                    let fr = (w.started.elapsed().as_secs_f32() * 10.0) as usize % n;
                    let b = gu.cell_bounds("anc     kaipage", fr, 0).unwrap_or([0.0; 4]);
                    gu.sprite("anc     kaipage", fr, cx + sx - (b[2] - b[0]) / 2.0, cy + sy - (b[3] - b[1]), colour);
                    if let Some((_, name)) = w.npc_names.iter().find(|(id, _)| (*id & 0x3ff) as u16 == index & 0x3ff) {
                        gu.shape_text(name, cx + 36.0, cy + 26.0, name_colour, false);
                    }
                }
            }
        }
        if let Some((t, at)) = w.clock {
            let m = crate::dat::weather::vana_minute(t, at.elapsed().as_secs_f32()) as u32;
            self.grid_wait -= ticks.max(1.0);
            if self.grid_wait < 0.0 {
                self.grid_wait = 30.0;
                self.grid = super::map::grid_text(w, gu.map_regions());
            }
            gu.shape_text(&format!("{}:{:02}  {}", m / 60, m % 60, self.grid), cx - 50.0, cy + 26.0, tint, false);
            // Day orb: framesus table 0x10379950 by weekday ((t % 27648) / 3456).
            let secs = t as u64 + at.elapsed().as_secs();
            let day = ((secs % 27648) / 3456) as usize;
            let orb = [0x6a, 0x6d, 0x6f, 0x6c, 0x6b, 0x6e, 0x70, 0x71][day.min(7)];
            gu.sprite("menu    framesus", orb, cx - 68.0, cy + 26.0, tint);
            // Weather badge: usgaiji 8 + (weather - 4) / 2 -> table 0, 5, 3, 2, 1, 4, 6, 7; a second for doubles.
            if (4..20).contains(&w.weather) {
                let b = [0, 5, 3, 2, 1, 4, 6, 7][((w.weather - 4) / 2) as usize];
                gu.sprite("font    usgaiji", b, cx - 68.0, cy - 42.0, tint);
                if w.weather % 2 == 1 {
                    let bw = gu.sprite_width("font    usgaiji", b);
                    gu.sprite("font    usgaiji", b, cx - 68.0 + bw, cy - 42.0, tint);
                }
            }
        }
    }
}

/// The offscreen top bar (topbar-shot): the bars at a fade state, the main menu on child `menu`, the party
/// row at `hpp` and a target window with ("mob") or without ("npc") the gauge.
#[allow(clippy::too_many_arguments)]
pub fn topbar_shot(out: &std::path::Path, fade: &str, menu: Option<u16>, target: Option<&str>, hpp: u8, map: Option<&str>, downloading: Option<usize>, window: Option<&str>, config: Option<&str>, cfgset: Option<&str>, installed: Option<u32>, width: u32, height: u32) -> anyhow::Result<()> {
    use crate::render::screens::HudState;
    let cfg = crate::config::Config::load();
    let index = crate::dat::DatIndex::open(&cfg.ffxi_dir())?;
    let dll = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?;
    let mut gu = GameUi::new(super::game_ui::GameUiData::load(&index, &dll)?);
    let (sw, sh) = (width as f32, height as f32);
    let mut top = HudTop::default();
    let n = |s: &str| s.parse::<i32>().unwrap_or(10).clamp(1, 20);
    top.fade.fade = match fade.split_once(':') {
        Some(("in", v)) => Fade::In(n(v)),
        Some(("out", v)) => Fade::Out(n(v)),
        _ if fade == "hidden" => Fade::Hidden,
        _ => Fade::Shown,
    };
    top.set_net(100, 180, 326);
    let items = gu.menu_items(sw, sh, 0);
    let mut bars = BarText::default();
    if let Some(id) = menu {
        let [help, title] = gu.focus_text("menu    menuwind", id);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    }
    let st = HudState {
        party_anim: Vec::new(),
        target_anim: None,
        party: vec![(hpp as f32 / 100.0, 0.0)],
        party_window: "menu    ptw0".into(),
        party_away: Vec::new(),
        party_marks: Vec::new(),
        target: target.map(|t| (t != "npc").then_some(0.7)),
        menu_selected: menu.and_then(|id| items.iter().find(|(i, _)| *i as u16 + 1 == id)).map(|(i, _)| *i),
        menu_page: 0,
        menu_focused: true,
        chat_log: false,
    };
    gu.clear();
    let login = config == Some("login");
    let lobby = login || config.is_some_and(|c| c.starts_with("lobycwin") || c.starts_with("lobby"));
    if login {
        let input = super::UiInput { pointer: None, moved: false, clicked: false, events: Vec::new() };
        let (mut a, mut b, mut c, mut d) = ("127.0.0.1".to_string(), "tuxedo".to_string(), "secret".to_string(), String::new());
        super::login_window(&mut gu, &input, [width, height], [&mut a, &mut b, &mut c, &mut d], &mut 0, false, "", &cfg.client_version);
    } else if lobby {
        let slots = gu.title(sw, sh, Some(3), 0xfff, [true; 5]);
        let _ = slots;
    }
    if let Some(n) = downloading {
        super::downloading(&mut gu, n, sw, sh);
        gu.save_png(width, height, [0.0, 0.0, 0.0], out)?;
        println!("wrote {} (downloading draw {n})", out.display());
        return Ok(());
    }
    // --map ZONE,X,Z,overlay: the map overlay (extra) at 60 %, the player facing north.
    if let Some(spec) = map.filter(|s| s.ends_with(",overlay")) {
        let f: Vec<&str> = spec.split(',').collect();
        let num = |i: usize| f.get(i).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        let region = super::map::Region::find(gu.map_regions(), num(0) as u16);
        let mut o = super::map::Overlay::new(num(0) as u16, region, Some(std::sync::Arc::new(index)));
        o.draw(&mut gu, (num(1), num(2)), std::f32::consts::FRAC_PI_2, sw, sh, 0.6);
        o.wait_image(&mut gu);
        gu.clear();
        o.draw(&mut gu, (num(1), num(2)), std::f32::consts::FRAC_PI_2, sw, sh, 0.6);
        gu.save_png(width, height, [0.1, 0.2, 0.3], out)?;
        println!("wrote {} (map overlay)", out.display());
        return Ok(());
    }
    // --map ZONE,X,Z[,owned]: the map window as the main menu's Map opens it, the player facing north.
    if let Some(spec) = map {
        let f: Vec<&str> = spec.split(',').collect();
        let num = |i: usize| f.get(i).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        let zone = num(0) as u16;
        // FUN_101c2090(zone, 1): group 0x14, the English places file.
        let place = crate::dat::names::DMsg::load(&index, crate::dat::names::PLACES[1])?.text(zone as usize).filter(|n| !n.is_empty());
        let region = super::map::Region::find(gu.map_regions(), zone).ok_or_else(|| anyhow::anyhow!("no map record for zone {zone}"))?;
        let owned = f.get(3) == Some(&"owned");
        gu.open_ticks = 0.0;
        let zoom = f.get(4).and_then(|v| v.parse().ok()).unwrap_or(2000);
        let mut m = super::map::MapWin::new(region, owned, Some(std::sync::Arc::new(index)), (num(1), num(2)), std::f32::consts::FRAC_PI_2, sw, sh, super::map::Mode::Menu, zoom, true);
        m.wait_image(&mut gu);
        if f.get(5) == Some(&"markers") {
            m.shot_markers();
        }
        m.draw(&mut gu, sw, sh);
        let [help, title] = gu.focus_text("menu    map0", m.cursor);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
        match place {
            Some(n) => bars.help[2] = n,
            None => log::error!("no place name (group 0x14) for zone {zone}"),
        }
    }
    let hud = if lobby { Vec::new() } else { gu.hud(sw, sh, &st) };
    for t in hud {
        match t.what {
            "target name" => {
                gu.shape_text(if target == Some("npc") { "Home Point #2" } else { "Brutal Sheep" }, t.x, t.y, 0x8080_8080, false);
            }
            "member name" => {
                gu.shape_name("Pletsay", t.x, t.y, 0x8080_8080);
                // party_tp: FUN_101f9940 at (+0x30, +10), blue from 1000.
                gu.shape_text("1250", t.x + 48.0, t.y + 10.0, 0x8020_60d0, true);
            }
            "HP" => {
                let tint = gu.hp_tint(hpp);
                gu.shape_text("658", t.x, t.y, tint, true);
            }
            "MP" => {
                gu.shape_text("0", t.x, t.y, 0x8080_8080, true);
            }
            _ => {}
        }
    }
    for w in window.unwrap_or_default().split(',').filter(|w| !w.is_empty()) {
        // "iteminfo:ID[:PAGE[:EXTHEX]]": the item info window as an event opens it.
        if let Some(rest) = w.strip_prefix("iteminfo:") {
            let f: Vec<&str> = rest.split(':').collect();
            let id = f[0].strip_prefix("0x").map_or_else(|| f[0].parse().ok(), |h| u32::from_str_radix(h, 16).ok()).unwrap_or(0);
            let page = f.get(1).and_then(|p| p.parse().ok()).unwrap_or(0);
            let ext: Option<[u8; 24]> = f.get(2).map(|h| std::array::from_fn(|k| h.get(k * 2..k * 2 + 2).and_then(|b| u8::from_str_radix(b, 16).ok()).unwrap_or(0)));
            gu.open_ticks = 0.0;
            if let Some(it) = crate::dat::DatIndex::open(&cfg.ffxi_dir()).ok().and_then(|i| crate::dat::items::Items::load(&i, crate::dat::items::Lang::En).ok()).and_then(|t| t.item(id)) {
                let aug = gu.aug_ctx();
                let info = super::iteminfo::build(&it, ext.as_ref(), &Default::default(), &|g, k| gu.item_msg(g, k), &|s| gu.text_width(s), aug.as_ref());
                println!("pages {:?}", info.pages);
                let layout = super::iteminfo::LAYOUTS[super::iteminfo::rows(&info, page) - 3];
                if let Some(r) = gu.window_swapped("menu    iteminfo", layout, sw, sh, (0.0, 0.0)) {
                    let icon = crate::render::invwin::icon_texture(id as u16);
                    if let Some(rgba) = it.icon_rgba().map(|i| i.2) {
                        gu.image(&icon, 32, 32, rgba);
                    }
                    super::iteminfo::draw(&mut gu, &info, page, Some(icon.as_str()), r[0], r[1]);
                }
            }
            continue;
        }
        // "passinpu:KIND:TEXT": the event input window with the caret at the end.
        if let Some(rest) = w.strip_prefix("passinpu:") {
            let (kind, text) = rest.split_once(':').unwrap_or((rest, ""));
            gu.open_ticks = 0.0;
            super::draw_event_input(&mut gu, kind.parse().unwrap_or(0), text, text.len(), sw, sh);
            continue;
        }
        let (name, sel) = w.split_once(':').unwrap_or((w, "0"));
        let name = format!("menu    {name}");
        let sel = sel.parse().unwrap_or(0);
        if gu.menu_sel(&name, sw, sh, sel).is_none() {
            log::error!("no menu {name}");
        }
        let [help, title] = gu.focus_text(&name, sel);
        (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
    }
    if let Some(spec) = config.filter(|_| !login) {
        let mut v = crate::gamecfg::Values::default();
        for kv in cfgset.unwrap_or_default().split(',').filter(|s| !s.is_empty()) {
            if let Some((k, x)) = kv.split_once('=') {
                let num = |s: &str| s.strip_prefix("0x").map_or_else(|| s.parse::<i64>().ok(), |h| i64::from_str_radix(h, 16).ok());
                if let (Some(k), Some(x)) = (num(k), num(x)) {
                    v.set(k as u16, x as u32);
                }
            }
        }
        let mut ui = super::configui::ConfigUi::default();
        ui.owned = installed.unwrap_or_else(|| crate::dat::installed(&cfg.ffxi_dir()));
        let mut settings = None;
        let mut update = None;
        for w in spec.split(',').filter(|w| !w.is_empty()) {
            let f: Vec<&str> = w.split(':').collect();
            // "update[:CURSOR]": the Update window with a sample plan (5321 files, 452 MB).
            if f[0] == "update" {
                let entry = crate::patch::Entry { path: "ROM/0/0.DAT".into(), versions: Vec::new() };
                let todo = vec![(0usize, crate::patch::Action::Direct, 84_910u64); 5321];
                let plan = crate::patch::Plan { server: crate::patch::ServerVersion { released: 0, label: "empty".into(), version: "30261006_0".into() }, manifest: Vec::new(), entries: vec![entry], todo, current: 44082, skipped_code: 10 };
                let status = std::sync::Arc::new(std::sync::Mutex::new(super::updater::Status { game: super::updater::Game::Available(plan), client: super::updater::Client::Unavailable(String::new()) }));
                let mut u = super::updater::UpdateUi::open(status, cfg.ffxi_dir());
                for _ in 0..f.get(1).and_then(|x| x.parse::<usize>().ok()).unwrap_or(0) {
                    u.key(egui::Key::ArrowDown);
                }
                update = Some(u);
                continue;
            }
            if f[0] == "settings" {
                let mut s = super::settings::SettingsUi::open(&cfg, [width, height]);
                s.gpus = crate::render::adapter_names(&wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle()), None);
                s.s.gpu = s.gpus.first().cloned().unwrap_or_default();
                s.goto(f.get(1).and_then(|x| x.parse().ok()).unwrap_or(0), f.get(2).and_then(|x| x.parse().ok()).unwrap_or(1));
                settings = Some(s);
                continue;
            }
            if f[0] == "lobby" {
                continue;
            }
            ui.push(f[0]);
            if let (Some(c), Some(win)) = (f.get(1).and_then(|x| x.parse().ok()), ui.stack.last_mut()) {
                win.cursor = c;
            }
            if let Some(s) = f.get(2).and_then(|x| x.parse().ok()) {
                ui.extra.scroll = s;
            }
        }
        ui.draw(&mut gu, &v, sw, sh);
        if let Some((name, child)) = ui.focus() {
            let [help, title] = gu.focus_text(&name, child);
            (bars.help[1], bars.title[1]) = (help.unwrap_or_default(), title.unwrap_or_default());
        }
        if let Some(s) = &settings {
            s.draw(&mut gu, sw, sh);
            (bars.help[1], bars.title[1]) = (s.help(), "Settings".into());
        }
        if let Some(u) = &update {
            u.draw(&mut gu, sw, sh);
            (bars.help[1], bars.title[1]) = (u.help(), "Update".into());
        }
    }
    if !lobby {
        top.draw_bars(&mut gu, sw, sh, shown(&bars.title), shown(&bars.help), 0.0, window == Some("busy"));
    }
    gu.save_png(width, height, [0.30, 0.36, 0.28], out)?;
    println!("wrote {} (fade {:?}, alpha {:.2}, title {:?}, help {:?})", out.display(), top.fade.fade, top.fade.alpha(), shown(&bars.title), shown(&bars.help));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(f: &mut TopFade, n: usize, active: bool) {
        for _ in 0..n {
            f.tick(active, true);
        }
    }

    #[test]
    fn starts_up_then_fades() {
        let mut f = TopFade::default();
        run(&mut f, 29, false);
        assert_eq!((f.fade, f.timer), (Fade::Shown, 1));
        run(&mut f, 1, false);
        assert_eq!((f.fade, f.timer), (Fade::Out(19), 0));
        run(&mut f, 18, false);
        assert_eq!(f.fade, Fade::Out(1));
        assert!((f.alpha() - 0.05).abs() < 1e-6);
        run(&mut f, 1, false);
        assert_eq!(f.fade, Fade::Hidden);
    }

    #[test]
    fn fade_in_and_hold() {
        let mut f = TopFade { fade: Fade::Hidden, timer: 0, acc: 0.0 };
        run(&mut f, 1, true);
        assert_eq!((f.fade, f.timer), (Fade::In(19), 180));
        run(&mut f, 19, true);
        assert_eq!(f.fade, Fade::Shown);
        // Active never counts down.
        run(&mut f, 1000, true);
        assert_eq!((f.fade, f.timer), (Fade::Shown, 180));
        run(&mut f, 179, false);
        assert_eq!(f.fade, Fade::Shown);
        run(&mut f, 1, false);
        assert_eq!(f.fade, Fade::Out(19));
    }

    #[test]
    fn timer_not_rearmed_while_up() {
        // Open, close for 120 ticks, reopen: the timer keeps 60, so the bars go 60 ticks after closing again.
        let mut f = TopFade { fade: Fade::Hidden, timer: 0, acc: 0.0 };
        run(&mut f, 30, true);
        run(&mut f, 120, false);
        run(&mut f, 30, true);
        assert_eq!(f.timer, 60);
        run(&mut f, 59, false);
        assert_eq!(f.fade, Fade::Shown);
        run(&mut f, 1, false);
        assert_eq!(f.fade, Fade::Out(19));
        // Text again in the middle of the fade-out: the timer is 0, so they fade straight back in.
        run(&mut f, 5, false);
        run(&mut f, 1, true);
        assert_eq!((f.fade, f.timer), (Fade::In(19), 180));
    }

    #[test]
    fn zoning_freezes_rule_and_ticks_are_whole() {
        let mut f = TopFade::default();
        f.advance(100.0, false, false);
        assert_eq!((f.fade, f.timer), (Fade::Shown, 30));
        f.advance(0.5, false, true);
        assert_eq!(f.timer, 30);
        f.advance(0.5, false, true);
        assert_eq!(f.timer, 29);
    }

    #[test]
    fn shown_slot_priority() {
        let mut t = BarText::default();
        t.title[0] = "Commands".into();
        assert_eq!(shown(&t.title), "Commands");
        t.title[1] = "Map".into();
        assert_eq!(shown(&t.title), "Map");
        t.title[3] = "News".into();
        assert_eq!(shown(&t.title), "News");
    }
}
