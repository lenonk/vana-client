//! The game's own 2D UI, drawn from its menu DATs and font: the lobby screens (title art, buttons,
//! expansion logos, help bar, credentials window) and the in-game HUD windows.

use crate::dat::DatIndex;
use crate::dat::font::{Font, Quality, encode_sjis};
use crate::dat::msg::{Messages, group_file};
use crate::dat::ui::{LOBBY_US, SpriteLib, UiFile, WINDOW_STYLE_BASE, companions};
use crate::render::Gpu;
use crate::render::screens::{self, HudState, LOBBY_CLEAR, TextSlot, TitleState};
use crate::render::ui::{UiBatch, UiRenderer};
use anyhow::{Context, Result};
use std::time::Instant;

/// Texture name the font atlas is uploaded under (cannot clash with a DAT name).
const ATLAS: &str = "\0font atlas";
/// Glyph colours, BGRA with 0x80 = 1.0.
pub const WHITE: [u8; 4] = [0x80; 4];
pub const YELLOW: [u8; 4] = [0x30, 0x78, 0x80, 0x80];
pub const GREY: [u8; 4] = [0x58, 0x58, 0x58, 0x80];

/// Parsed resources; loaded off the UI thread.
pub struct GameUiData {
    lobby: UiFile,
    style: SpriteLib,
    font: Font,
    /// Message group 0xe: menu help lines and prompts ("Log out?").
    menu_msgs: Option<Messages>,
    /// Message group 0xd (file 55652): window titles ("Items", "Commands").
    title_msgs: Option<Messages>,
    /// Action command help ids (group 0xe), u16 at 0x103634fc + 12 * command.
    cmd_help: Vec<u16>,
    /// The map region table (0x10376010).
    map_regions: Vec<super::map::Region>,
    region_groups: Vec<(u8, Vec<u16>)>,
    /// File slot 0x1c (55688 in English): item info formats.
    item_fmt: Option<Messages>,
    /// Augments: texts (slot 0x1b, 55692), server names (group 0x16), the DLL tables, file 81's mnc2 chunk.
    aug_text: Option<Messages>,
    servers: Option<Messages>,
    aug_tables: Option<super::augment::AugTables>,
    mnc2: Option<Vec<u8>>,
    /// File 81's `mon_` chunk: the species list order (codes, the zeros dropped).
    pub mon_order: Vec<u16>,
    /// Lobby message groups 1 (Face/Hair/Size), 6 (race names), 8 (lobby help), 0x1b (name help).
    lobby_msgs: std::collections::HashMap<u8, Messages>,
    /// Creator base stats, 8 races x 6 jobs x 7 bytes (FFXiMain 0x1037bb10, FUN_102306d0).
    stats: Vec<u8>,
    /// Group 4: short area names (file 0xd8aa, loaded by FUN_1014d2b0 for every language).
    area_names: Option<Messages>,
    /// FFXI error popups (FUN_10017310): {code, kind, group, id} records at 0x10345800, up to code 9999.
    errors: Vec<[u32; 4]>,
    /// The select screen's count lines (FUN_102376b0, English): purchased, registered, more (plural, singular).
    count_fmt: [String; 4],
    /// Window frame styles 0..7 = files 14..21 (FUN_10159a70, config 0x12).
    styles: Vec<UiFile>,
    /// The status info files (FUN_10227280): slot 0x3a (32x32 icons) and 0x3b (16x16), config 0x99 bit 2 picking.
    status: [Option<std::sync::Arc<Vec<u8>>>; 2],
    /// The menu registry's flags by window name (0x2c-byte entries from "menu    menuwind"; FUN_1015e950 copies them
    /// to window +0x34): 8 = no open / close animation, 0x20 frameless. Unlisted windows are 0.
    registry: std::collections::HashMap<String, u32>,
    /// The registry's group masks (u16 +0x2a, window +0x38): FUN_1015fa00 suspends a window whose bits a window
    /// above it holds.
    groups: std::collections::HashMap<String, u16>,
}

/// The menu registry in FFXiMain: {name[32], handler, u32 flags, ...} records 0x2c apart from "menu    menuwind" to
/// an empty name. Found as the copy of that name followed by another "menu    " entry.
fn menu_registry(ffximain: &[u8]) -> (std::collections::HashMap<String, u32>, std::collections::HashMap<String, u16>) {
    let (mut out, mut groups) = (std::collections::HashMap::new(), std::collections::HashMap::new());
    let first = b"menu    menuwind";
    let Some(at) = (0..ffximain.len().saturating_sub(0x60)).find(|&i| ffximain[i..].starts_with(first) && ffximain[i + 16] == 0 && ffximain[i + 0x2c..].starts_with(b"menu    ")) else {
        log::warn!("menu registry not found");
        return (out, groups);
    };
    for e in ffximain[at..].chunks_exact(0x2c) {
        let name = &e[..32];
        let len = name.iter().position(|&c| c == 0).unwrap_or(32);
        if len == 0 {
            break;
        }
        // Keyed like the layout names (trimmed, lower case), so the padded short names match too.
        let key = crate::dat::ui::norm_name(&name[..len]);
        groups.insert(key.clone(), u16::from_le_bytes([e[0x2a], e[0x2b]]));
        out.insert(key, u32::from_le_bytes(e[0x24..0x28].try_into().unwrap()));
    }
    (out, groups)
}

// From FFXiMain (dat::tables): the creator stat table (STR DEX VIT AGI INT MND CHR per race and job), the action
// command records (12 bytes, the help id at +4), the error table, and the count formats ("No. of content IDs
// purchased: %2d", "No. of characters registered: %2d", "(%d more characters can be made.)", the singular).

/// A NUL-terminated string in FFXiMain's data.
fn va_str(ffximain: &[u8], va: u32) -> String {
    let b = crate::dat::battle::va_bytes(ffximain, va, 64).unwrap_or_default();
    crate::dat::font::decode_sjis(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())])
}

impl GameUiData {
    pub fn load(index: &DatIndex, ffximain: &[u8]) -> Result<Self> {
        Self::load_with(index, ffximain, Quality::Compressed)
    }

    /// With the font quality of Config-tool 0036.
    pub fn load_with(index: &DatIndex, ffximain: &[u8], quality: Quality) -> Result<Self> {
        let parse = |id: u32| -> Result<UiFile> { UiFile::parse(&index.read(id)?) };
        let mut lobby = parse(LOBBY_US)?;
        for &c in companions(LOBBY_US) {
            lobby.absorb(parse(c)?, true);
        }
        match parse(crate::dat::ui::MAP_BACKDROP) {
            Ok(f) => lobby.absorb(f, true),
            Err(e) => log::warn!("map backdrop: {e:#}"),
        }
        // FUN_1011ad50 (non-Japanese clients, called by the dead windows' open): the window takes its frame sprite's
        // bounds, framesus 65's 144 x 44 for "menu    dead".
        for name in ["menu    dead", "menu    mp_dead"] {
            let size = lobby.menu(name).and_then(|m| m.elements[0].refs.iter().find(|r| r.kind == 0)).and_then(|r| {
                let s = lobby.lib(&r.lib)?.sprites.get(r.index as usize)?;
                let w = s.cells.iter().flat_map(|c| c.corners.iter().map(|p| p[0])).max()?;
                let h = s.cells.iter().flat_map(|c| c.corners.iter().map(|p| p[1])).max()?;
                Some((w.max(0) as u16, h.max(0) as u16))
            });
            if let (Some((w, h)), Some(m)) = (size, lobby.menus.iter_mut().find(|m| m.name == name)) {
                (m.elements[0].w, m.elements[0].h) = (w, h);
            }
        }
        let style = parse(WINDOW_STYLE_BASE)?.libs.into_iter().next().context("window style file has no sprite library")?;
        let font = Font::load(index, ffximain, 1, quality)?;
        let t = crate::dat::tables::of(ffximain)?;
        let menu_msgs = group_file(ffximain, 1, 0x0e).and_then(|f| Messages::load(index, f).map_err(|e| log::warn!("menu messages: {e:#}")).ok());
        let title_msgs = group_file(ffximain, 1, 0x0d).and_then(|f| Messages::load(index, f).map_err(|e| log::warn!("title messages: {e:#}")).ok());
        let cmd_help = crate::dat::battle::va_bytes(ffximain, t.cmd_help, 0x1c * 12).map_or(Vec::new(), |b| b.chunks(12).map(|r| u16::from_le_bytes([r[0], r[1]])).collect());
        let lobby_msgs = [1u8, 3, 6, 7, 8, 9, 10, 0x0b, 0x0c, 0x12, 0x13, 0x15, 0x1b]
            .into_iter()
            .filter_map(|g| Some((g, group_file(ffximain, 1, g).and_then(|f| Messages::load(index, f).map_err(|e| log::warn!("message group {g:#x}: {e:#}")).ok())?)))
            .collect();
        let stats = crate::dat::battle::va_bytes(ffximain, t.stats, 8 * 6 * 7).map(<[u8]>::to_vec).unwrap_or_default();
        let map_regions = super::map::regions(ffximain);
        let region_groups = super::map::region_groups(ffximain);
        let aug_text = crate::dat::msg::slot_file(ffximain, 1, 0x1b).and_then(|f| Messages::load(index, f).map_err(|e| log::warn!("augment texts: {e:#}")).ok());
        let servers = crate::dat::msg::group_file(ffximain, 1, 0x16).and_then(|f| Messages::load(index, f).ok());
        let aug_tables = super::augment::AugTables::find(ffximain);
        let f81 = index.read(81).ok();
        let mnc2 = f81.as_deref().and_then(super::augment::mnc2);
        let mon_order = f81.as_deref().map(super::monstrosity::order).unwrap_or_default();
        let item_fmt = crate::dat::msg::slot_file(ffximain, 1, 0x1c).and_then(|f| Messages::load(index, f).map_err(|e| log::warn!("item info formats: {e:#}")).ok());
        let area_names = Messages::load(index, 0xd8aa).map_err(|e| log::error!("area names (group 4): {e:#}")).ok();
        let errors = crate::dat::battle::va_bytes(ffximain, t.errors, 16 * 128)
            .map(|b| b.chunks(16).map(|r| std::array::from_fn(|k| u32::from_le_bytes(r[k * 4..k * 4 + 4].try_into().unwrap()))).take_while(|r: &[u32; 4]| r[0] != 9999).collect())
            .unwrap_or_default();
        let count_fmt = t.count_fmt.map(|va| va_str(ffximain, va));
        let styles = (0..8).filter_map(|k| parse(WINDOW_STYLE_BASE + k).map_err(|e| log::warn!("window style {k}: {e:#}")).ok()).collect();
        let (registry, groups) = menu_registry(ffximain);
        let status = [0x3a, 0x3b].map(|slot| crate::dat::msg::current_slot_file(ffximain, 1, slot).and_then(|f| index.read(f).map_err(|e| log::warn!("status info {f}: {e:#}")).ok()).map(std::sync::Arc::new));
        Ok(Self { lobby, style, font, menu_msgs, title_msgs, cmd_help, map_regions, region_groups, item_fmt, aug_text, servers, aug_tables, mnc2, mon_order, lobby_msgs, stats, area_names, errors, count_fmt, styles, status, registry, groups })
    }
}

impl GameUi {
    /// "menu    dmgnum" (file 39542) and its texture: the on-screen damage number glyphs.
    pub fn dmgnum(&self) -> Option<(&SpriteLib, &crate::dat::ui::UiImage)> {
        let lib = self.data.lobby.lib("menu    dmgnum")?;
        let tex = lib.sprites.first()?.cells.first()?.texture.as_str();
        Some((lib, self.data.lobby.images.get(tex)?))
    }
}

pub struct GameUi {
    data: GameUiData,
    renderer: Option<UiRenderer>,
    /// The scene depth name plates are tested against this frame (taken by render).
    pub scene_depth: Option<wgpu::TextureView>,
    /// What this frame draws, rebuilt by the UI code each frame.
    batch: UiBatch,
    started: Instant,
    /// Images (item icons) waiting for upload, and the names already uploaded.
    pending: Vec<(String, u32, u32, Vec<u8>)>,
    images: std::collections::HashSet<String>,
    help: HelpScroll,
    /// Party members the last HUD drew (the target window stacks on the party window).
    party_window: String,
    /// Region 0's top, the chat log's frame (set each world frame): registry flag 0x1000 windows stack on it.
    pub log_top: Option<f32>,
    /// The log shows no lines (fading or faded): stacked windows sit on its bottom instead.
    pub log_faded: bool,
    /// The target window is up this frame (region 2's window, which flag-0x4000 windows sit on).
    target_up: bool,
    /// The window style in use and images to replace in the renderer.
    style_index: usize,
    replaced: Vec<(String, u32, u32, Vec<u8>)>,
    /// The startup images a style replaced, for reset_style.
    style_saved: std::collections::HashMap<String, crate::dat::ui::UiImage>,
    /// When each scrolling text (scroll_text) first showed.
    scroll_seen: std::collections::HashMap<String, Instant>,
    /// The pointer is over an enabled item (mouse cursor kind 1); set by the windows' hit tests, cleared when the
    /// pointer moves.
    pub hover: std::cell::Cell<bool>,
    /// The focused lists' bars drawn this frame, and a press held on one (client/scrollbar.rs).
    pub bars: Vec<super::scrollbar::Site>,
    pub bar_hold: Option<super::scrollbar::Hold>,
    /// Window open / close animations by menu name, the UI frame number and the tick clock; `open_ticks` is N
    /// (DAT_1036c190: 25, 0 with config 0xa4).
    anims: std::collections::HashMap<String, WinAnim>,
    /// What a window that swaps its contents in place showed last (replay_on_change).
    contents: std::collections::HashMap<String, u64>,
    anim_frame: u64,
    anim_acc: f32,
    pub open_ticks: f32,
    walk: Walk,
    /// The window drawn with the hand and its cursor child's rect, this frame and the last (what Enter presses).
    focus: (Option<(String, u16, [f32; 4])>, Option<(String, u16, [f32; 4])>),
    /// Window +0x5e, the generic press (FUN_10118260 on Enter: 9 ticks down): the child pressed and the count.
    presses: std::cell::RefCell<std::collections::HashMap<String, (u16, f32)>>,
}

/// FUN_1015fa00's group walk (window_groups_spec.md): the windows wanted this frame and last with their raise
/// stamps, and those the walk suspended for this frame.
#[derive(Default)]
struct Walk {
    now: std::collections::HashMap<String, i64>,
    last: std::collections::HashMap<String, i64>,
    suspended: std::collections::HashSet<String>,
    top: i64,
    /// Group overrides (GameUi::set_group) and window flags set while open (GameUi::force_flags).
    groups: std::collections::HashMap<String, u16>,
    flags: std::collections::HashMap<String, u32>,
}

/// One window's open / close state (window +0x26 open count, +0x64 close count, +0x68 opened animated).
struct WinAnim {
    open: f32,
    close: f32,
    animated: bool,
    shape: Shape,
    seen: u64,
}

/// What zooms: a framed window's frame rect, or a frameless window's sprite (library, index) at its position.
#[derive(Clone)]
enum Shape {
    Frame([f32; 4]),
    Sprite(String, usize, f32, f32),
}

/// The lobby help line's scroll state (lobyhelp draw FUN_10236b40).
#[derive(Default)]
struct HelpScroll {
    text: String,
    /// Frames left before scrolling starts (0x1e on a new message), pixels scrolled, frame time owed.
    wait: u32,
    offset: f32,
    acc: f32,
    last: Option<Instant>,
}

/// A drawn lobby window: its frame rect and its children's rects by id.
pub struct LobbyWin {
    pub frame: [f32; 4],
    pub children: Vec<(u16, [f32; 4])>,
}

/// What the creator screen shows for the current choices.
pub struct CreatorView<'a> {
    /// 0 race .. 5 name, 6 nation, 7 confirm.
    pub step: usize,
    pub race: usize,
    /// Face 0..7 and hair 0 (A) / 1 (B).
    pub face: usize,
    pub hair: usize,
    pub size: usize,
    pub job: usize,
    pub nation: usize,
    pub name: &'a str,
    /// The world step over the name step (creator::WorldStep).
    pub world: Option<WorldView<'a>>,
    /// chmkname closed: the name step's checks and their popups (FUN_10239720 step 7).
    pub name_hidden: bool,
}

/// What the creator's world step shows.
pub struct WorldView<'a> {
    pub rows: &'a [String],
    pub cursor: usize,
    pub top: usize,
    /// Phase 1: the pass, its caret and whether the caret shows.
    pub pass: Option<(&'a str, usize, bool)>,
}

impl GameUi {
    pub fn new(data: GameUiData) -> Self {
        Self { data, renderer: None, scene_depth: None, batch: UiBatch::default(), started: Instant::now(), pending: Vec::new(), images: Default::default(), help: HelpScroll::default(), party_window: "menu    ptw0".into(), log_top: None, style_index: 0, replaced: Vec::new(), style_saved: Default::default(), scroll_seen: Default::default(), hover: Default::default(), bars: Vec::new(), bar_hold: None, anims: Default::default(), contents: Default::default(), anim_frame: 1, anim_acc: 0.0, open_ticks: 25.0, log_faded: false, target_up: false, walk: Walk::default(), focus: (None, None), presses: Default::default() }
    }

    /// FUN_1015e530 / FUN_101177d0 for a window drawn this frame with frame rect `r`: false while it opens (only the
    /// zooming frame is drawn, nothing of its contents), true once it is open.
    fn win_gate(&mut self, name: &str, r: [f32; 4]) -> bool {
        self.gate(name, Shape::Frame(r))
    }

    fn gate(&mut self, name: &str, shape: Shape) -> bool {
        // A suspended window is not drawn: its close plays, and its open once the walk lets it back.
        if !self.want(name) {
            return false;
        }
        let n = self.open_ticks;
        let animates = n > 0.0 && self.data.registry.get(name.trim_end().to_ascii_lowercase().as_str()).copied().unwrap_or(0) & 8 == 0;
        let frame = self.anim_frame;
        let a = self.anims.entry(name.to_string()).or_insert(WinAnim { open: 0.0, close: 0.0, animated: false, shape: shape.clone(), seen: 0 });
        if a.seen == 0 || a.seen + 1 < frame {
            (a.open, a.close, a.animated) = (if animates { n } else { 0.0 }, 0.0, animates);
        }
        (a.seen, a.shape) = (frame, shape.clone());
        if a.open <= 0.0 {
            return true;
        }
        let f = ((n - a.open) / n).max(0.0);
        self.zoom(&shape, f);
        false
    }

    fn zoom(&mut self, shape: &Shape, f: f32) {
        match shape {
            Shape::Frame(r) => self.zoom_frame(*r, f),
            Shape::Sprite(lib, k, x, y) => self.zoom_sprite(lib, *k, *x, *y, f),
        }
    }

    /// FUN_1011a150: a frameless window's sprite, every cell, at scale f^2 and an echo at 1.6 f with alpha
    /// min(0x80, ftol(128 - 128 f)), each moved by |bounds| (s - 1) / 2 (FUN_1011fc80's centring).
    fn zoom_sprite(&mut self, lib: &str, k: usize, x: f32, y: f32, f: f32) {
        let Some(l) = self.data.lobby.lib(lib) else { return };
        let Some(sp) = l.sprites.get(k) else { return };
        let (mut x0, mut y0, mut x1, mut y1) = (i16::MAX, i16::MAX, i16::MIN, i16::MIN);
        for c in sp.cells.iter().flat_map(|c| c.corners) {
            (x0, y0, x1, y1) = (x0.min(c[0]), y0.min(c[1]), x1.max(c[0]), y1.max(c[1]));
        }
        let (bw, bh) = ((x1 as f32 - x0 as f32).abs(), (y1 as f32 - y0 as f32).abs());
        let at = |s: f32| (x - (bw * (s - 1.0) * 0.5).trunc(), y - (bh * (s - 1.0) * 0.5).trunc());
        let s = f * f;
        let (ax, ay) = at(s);
        self.batch.push_sprite(&self.data.lobby, l, k, ax, ay, s, s, 0x8080_8080, 0);
        let g = 1.6 * f;
        let alpha = ((128.0 - 128.0 * f) as i32).clamp(0, 0x80) as u32;
        let (bx, by) = at(g);
        self.batch.push_sprite(&self.data.lobby, l, k, bx, by, g, g, alpha << 24 | 0x80_8080, 0x44);
    }

    /// FUN_1011a250: the style frame at scale f^2 about its centre, then an echo at 1.6 f, alpha blended with alpha
    /// ftol(96 - 128 f) clamped unsigned to 0x80 (so it turns opaque past f = 0.75, as retail's compare does).
    fn zoom_frame(&mut self, r: [f32; 4], f: f32) {
        let [x, y, w, h] = r;
        let at = |s: f32| ((x - (w / 2.0).trunc() * (s - 1.0)).trunc(), (y - (h / 2.0).trunc() * (s - 1.0)).trunc());
        let s = f * f;
        let (ax, ay) = at(s);
        self.batch.push_frame(&self.data.lobby, &self.data.style, ax, ay, w, h, s, s, 0x8080_8080, 0, false, false);
        let g = 1.6 * f;
        let alpha = (96.0 - 80.0 * g) as i32 as u32;
        let alpha = if alpha > 0x80 { 0x80 } else { alpha };
        let (bx, by) = at(g);
        self.batch.push_frame(&self.data.lobby, &self.data.style, bx, by, w, h, g, g, alpha << 24 | 0x80_8080, 0x44, false, false);
    }

    /// Once a frame, after every window: open counts run down, windows no longer drawn play their close (the frame
    /// shrinking from the last rect, FUN_101187f0's count), finished ones go.
    pub fn end_frame(&mut self, dt: f32) {
        self.anim_acc += dt * 60.0;
        let ticks = self.anim_acc.floor();
        self.anim_acc -= ticks;
        let (n, frame) = (self.open_ticks, self.anim_frame);
        let mut closing = Vec::new();
        for a in self.anims.values_mut() {
            if a.seen == frame {
                a.open = (a.open - ticks).max(0.0);
                continue;
            }
            if a.seen + 1 == frame && a.animated && n > 0.0 && a.close <= 0.0 {
                a.close = n;
            }
            if a.close > 0.0 {
                closing.push((a.shape.clone(), a.close / n));
                a.close -= ticks;
            }
        }
        for (shape, f) in closing {
            self.zoom(&shape, f);
        }
        self.anims.retain(|_, a| a.seen == frame || a.close > 0.0);
        self.anim_frame += 1;
        self.run_walk();
        self.focus = (None, self.focus.0.take());
        self.presses.borrow_mut().retain(|_, p| {
            p.1 -= ticks;
            p.1 > 0.0
        });
    }

    /// The generic handler's Enter (FUN_10118260, message 5): the focused window's cursor child sinks to (+1, +1) and
    /// its two rings play for 9 ticks (FUN_101177d0 -> FUN_10119f80 with N = 20).
    pub fn press(&self) {
        if let Some((name, child, _)) = &self.focus.1 {
            self.presses.borrow_mut().insert(name.clone(), (*child, 9.0));
        }
    }

    /// A click on the focused window's cursor child is its Enter.
    pub fn press_at(&self, p: (f32, f32)) {
        if self.focus.1.as_ref().is_some_and(|(_, _, r)| p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3]) {
            self.press();
        }
    }

    /// A window that is open this frame (drawn or not); false while the walk has it suspended. Its raise stamp is
    /// kept while it stays open: a new one goes on top, except targetwi, which FUN_101590c0 reopens with defer: under
    /// the windows that hold its bits *(inf: gaugewin and playermo, which also defer, are taken as new windows)*.
    pub fn want(&mut self, name: &str) -> bool {
        let key = crate::dat::ui::norm_name(name.as_bytes());
        if !self.walk.now.contains_key(&key) {
            let stamp = match self.walk.last.get(&key) {
                Some(&s) => s,
                None => {
                    let g = self.win_group(&key);
                    let under = if key == "menu    targetwi" { self.walk.last.iter().filter(|(n, _)| self.win_group(n) & g != 0).map(|(_, &s)| s - 1).min() } else { None };
                    under.unwrap_or_else(|| {
                        self.walk.top += 1000;
                        self.walk.top
                    })
                }
            };
            self.walk.now.insert(key.clone(), stamp);
        }
        !self.walk.suspended.contains(&key)
    }

    /// The walk (0x1015fe10..0x1015fe9c), newest stamp first from an empty mask: a window sharing a bit with the
    /// mask is suspended and adds none of its bits; any other adds its group. Group 0 windows take no part.
    fn run_walk(&mut self) {
        let mut order: Vec<(String, i64)> = self.walk.now.drain().collect();
        order.sort_by_key(|o| std::cmp::Reverse(o.1));
        let mut mask = 0u16;
        self.walk.suspended.clear();
        for (name, _) in &order {
            let g = self.win_group(name);
            if g & mask != 0 {
                self.walk.suspended.insert(name.clone());
            } else {
                mask |= g;
            }
        }
        self.walk.last = order.into_iter().collect();
    }

    /// Text caret phase: on for half of each second.
    pub fn blink(&self) -> bool {
        self.started.elapsed().as_millis() % 1000 < 500
    }

    /// Hand cursor frame: six frames, one every 3 ticks at 60 Hz (FUN_10116c00).
    pub fn cursor_frame(&self) -> usize {
        (self.started.elapsed().as_secs_f32() * 20.0) as usize % 6
    }

    /// The title art; `owned` is the lobby's expansion bitmask (bit 1 Zilart .. bit 11 Adoulin).
    pub fn title(&mut self, w: f32, h: f32, selected: Option<usize>, owned: u32, enabled: [bool; 5]) -> Vec<TextSlot> {
        self.title_update(w, h, selected, owned, enabled, None, 0, None, true)
    }

    /// The title with the Update button above Select Character (selected, enabled).
    #[allow(clippy::too_many_arguments)]
    pub fn title_update(&mut self, w: f32, h: f32, selected: Option<usize>, owned: u32, enabled: [bool; 5], update: Option<(bool, bool)>, wardrobes: u32, press: Option<(f32, bool)>, hand: bool) -> Vec<TextSlot> {
        let st = TitleState { selected: selected.unwrap_or(0), owned: owned >> 1, cursor_frame: self.cursor_frame(), buttons: selected.is_some(), shift: Self::shift(owned), enabled, update, hand, wardrobes, press };
        let (b, text) = screens::title_screen(&self.data.lobby, &self.data.style, w, h, &st);
        self.batch = b;
        text
    }

    /// Without expansion logos the art is centred.
    pub fn shift(owned: u32) -> f32 {
        if owned >> 1 == 0 { screens::CENTRE_SHIFT } else { 0.0 }
    }

    pub fn button_rects(&self, w: f32, h: f32, owned: u32) -> Vec<[f32; 4]> {
        screens::title_button_rects(&self.data.lobby, w, h, Self::shift(owned))
    }

    pub fn update_button_rect(&self, w: f32, h: f32, owned: u32) -> Option<[f32; 4]> {
        screens::update_button_rect(&self.data.lobby, w, h, Self::shift(owned))
    }

    pub fn text_width(&self, s: &str) -> f32 {
        self.data.font.text_width(&encode_sjis(s)) as f32
    }

    /// One line in the game font; align 0 = left at x, 1 = centred on x.
    pub fn text(&mut self, s: &str, x: f32, y: f32, align: u8, color: [u8; 4]) {
        let sjis = encode_sjis(s);
        let x = if align == 1 { x - self.data.font.text_width(&sjis) as f32 / 2.0 } else { x };
        let mut quads = Vec::new();
        self.data.font.layout_line(&sjis, x.round() as i32, y.round() as i32, color, [1.0, 1.0], &mut quads);
        self.batch.push_text(&quads, ATLAS);
    }

    /// The select screen's 16-slot list (dbnamese); returns the slot rects and the list window's area.
    pub fn select_list(&mut self, w: f32, h: f32, cursor: usize, used: &[bool], focused: bool, press: Option<f32>) -> (Vec<[f32; 4]>, [f32; 4]) {
        let cursor_frame = self.cursor_frame();
        screens::select_list(&mut self.batch, &self.data.lobby, w, h, cursor, used, focused.then_some(cursor_frame), press)
    }

    /// Game-font text fitted into `max` px (FUN_101ca250): cut where 16 px plus the text so far would pass `max`,
    /// "..." drawn after the cut.
    pub fn text_fit(&mut self, s: &str, x: f32, y: f32, max: f32, tint: u32) {
        let mut used = 16.0;
        for (i, c) in s.char_indices() {
            let cw = self.text_width(c.encode_utf8(&mut [0; 4]));
            used += cw;
            if used > max {
                self.text(&s[..i], x, y, 0, tint.to_le_bytes());
                self.text("...", x + used - cw - 16.0, y, 0, tint.to_le_bytes());
                return;
            }
        }
        self.text(s, x, y, 0, tint.to_le_bytes());
    }

    /// Sprite-font text fitted into `max` px (FUN_101c04c0).
    pub fn shape_fit(&mut self, s: &str, x: f32, y: f32, max: f32, tint: u32) -> f32 {
        screens::push_shape_fit(&mut self.batch, &self.data.lobby, &encode_sjis(s), x, y, max, tint)
    }

    /// The window-style frame at (x, y) with an ARGB tint (FUN_1022a290).
    pub fn frame_tint(&mut self, x: f32, y: f32, w: f32, h: f32, tint: u32) {
        self.batch.push_frame(&self.data.lobby, &self.data.style, x, y, w, h, 1.0, 1.0, tint, 0, false, false);
    }

    /// A layout's cursor links of child `child` (up, down, ?, ?, right, left).
    pub fn menu_links(&self, name: &str, child: u16) -> Option<[u8; 6]> {
        self.data.lobby.menu(name)?.elements.iter().skip(1).find(|e| e.id == child).map(|e| e.links)
    }

    /// A lobby window as FUN_101144b0 draws it: style frame, the window sprite from its second cell, each child's
    /// sprite, child `sel` tinted 0x80805020 one pixel up-left with the hand when `hand`; `hide` children are
    /// skipped. `centred` adds ((W - 512) / 2, (H - 448) / 2) (FUN_101179a0).
    #[allow(clippy::too_many_arguments)]
    /// Disabled (state 5) children over a window drawn with them hidden (FUN_101177d0 / FUN_10119c00): the last ref of
    /// kind 0 or 4 wins; a kind-4 sprite at full tint (cursor 0x80805020 at +1 px), a kind-0 one dimmed to 0x30808080
    /// (cursor 0x40402810 at +1 px).
    pub fn disabled_children(&mut self, name: &str, x: f32, y: f32, off: &[u16], cursor: Option<u16>) {
        let Some(l) = self.layout(name).cloned() else { return };
        for e in l.elements.iter().skip(1).filter(|e| off.contains(&e.id)) {
            let Some(r) = e.refs.iter().rfind(|r| r.kind == 0 || r.kind == 4) else { continue };
            let on = cursor == Some(e.id);
            let tint = match (r.kind == 4, on) {
                (true, false) => 0x8080_8080,
                (true, true) => 0x8080_5020,
                (false, false) => 0x3080_8080,
                (false, true) => 0x4040_2810,
            };
            let d = if on { 1.0 } else { 0.0 };
            self.sprite(&r.lib, r.index as usize, x + e.x as f32 + d, y + e.y as f32 + d, tint);
            // The cursor rests on a disabled child too (FUN_10118870 does not skip them): the hand goes there.
            if on {
                self.cursor(x + e.x as f32, y + e.y as f32);
            }
        }
    }

    pub fn lobby_window(&mut self, name: &str, w: f32, h: f32, centred: bool, sel: Option<u16>, hand: bool, hide: &[u16]) -> Option<LobbyWin> {
        let off = if centred { (((w - 512.0) / 2.0).trunc(), ((h - 448.0) / 2.0).trunc()) } else { (0.0, 0.0) };
        self.lobby_window_off(name, w, h, off, sel, hand, hide)
    }

    /// `lobby_window` moved by `off` (region stacking).
    #[allow(clippy::too_many_arguments)]
    /// A window drawn from `layout` but opened and animated as `gate`: a window that swaps its layout in place
    /// (FUN_1011a8f0). Returns the frame rect.
    pub fn window_swapped(&mut self, gate: &str, layout: &str, w: f32, h: f32, off: (f32, f32)) -> Option<[f32; 4]> {
        let m = self.data.lobby.menu(layout)?;
        let (fx, fy, fw, fh) = screens::frame_rect(m, w, h, 1.0, 1.0);
        let r = [fx + off.0, fy + off.1, fw, fh];
        if !self.win_gate(gate, r) {
            return None;
        }
        let ui = &self.data.lobby;
        let m = ui.menu(layout)?;
        let (x, y) = screens::window_pos(m, w, h, 1.0, 1.0);
        self.batch.push_frame(ui, &self.data.style, r[0], r[1], fw, fh, 1.0, 1.0, 0x8080_8080, 0, false, false);
        if let Some(rf) = m.elements[0].refs.iter().find(|r| r.kind == 0)
            && let Some(lib) = ui.lib(&rf.lib)
        {
            self.batch.push_sprite_from(ui, lib, rf.index as usize, 1, x + off.0, y + off.1, 1.0, 1.0, 0x8080_8080, 0);
        }
        Some(r)
    }

    /// What augment decoding needs, when its texts and tables loaded.
    pub fn aug_ctx(&self) -> Option<super::augment::AugCtx<'_>> {
        Some(super::augment::AugCtx {
            tables: self.data.aug_tables.as_ref()?,
            mnc2: self.data.mnc2.as_deref(),
            text: self.data.aug_text.as_ref()?,
            fmt88: self.data.item_fmt.as_ref(),
            servers: self.data.servers.as_ref(),
        })
    }

    /// Item info texts: groups 9..0xc, and 0x1c for file slot 0x1c (55688 in English).
    pub fn item_msg(&self, group: u8, id: usize) -> Option<String> {
        let m = if group == 0x1c { self.data.item_fmt.as_ref() } else { self.data.lobby_msgs.get(&group) };
        m.and_then(|m| m.get(id)).filter(|s| !s.is_empty())
    }

    /// A lobby window in a rect of its own (FUN_1011a5a0 resized or moved it): the style frame over (x, y, w, h) and its
    /// window sprite at (x, y); no children.
    pub fn window_sized(&mut self, name: &str, x: f32, y: f32, w: f32, h: f32) -> bool {
        if !self.win_gate(name, [x, y, w, h]) {
            return false;
        }
        let ui = &self.data.lobby;
        let Some(m) = ui.menu(name) else { return false };
        self.batch.push_frame(ui, &self.data.style, x, y, w, h, 1.0, 1.0, 0x8080_8080, 0, false, false);
        if let Some(r) = m.elements[0].refs.iter().find(|r| r.kind == 0)
            && let Some(lib) = ui.lib(&r.lib)
        {
            self.batch.push_sprite_from(ui, lib, r.index as usize, 1, x, y, 1.0, 1.0, 0x8080_8080, 0);
        }
        true
    }

    pub fn lobby_window_off(&mut self, name: &str, w: f32, h: f32, off: (f32, f32), sel: Option<u16>, hand: bool, hide: &[u16]) -> Option<LobbyWin> {
        self.lobby_window_swaps(name, w, h, off, sel, hand, hide, &[])
    }

    /// `lobby_window_off` with children's own sprites replaced (FUN_1011ae50: (child, sprite in the same library)).
    #[allow(clippy::too_many_arguments)]
    pub fn lobby_window_swaps(&mut self, name: &str, w: f32, h: f32, off: (f32, f32), sel: Option<u16>, hand: bool, hide: &[u16], swaps: &[(u16, usize)]) -> Option<LobbyWin> {
        let swaps: Vec<(u16, Option<&str>, usize)> = swaps.iter().map(|&(c, k)| (c, None, k)).collect();
        self.lobby_window_lib_swaps(name, w, h, off, sel, hand, hide, &swaps)
    }

    /// `lobby_window_swaps` where a swap may take its sprite from another library (FUN_1011ae50 with another table).
    #[allow(clippy::too_many_arguments)]
    pub fn lobby_window_lib_swaps(&mut self, name: &str, w: f32, h: f32, off: (f32, f32), sel: Option<u16>, hand: bool, hide: &[u16], swaps: &[(u16, Option<&str>, usize)]) -> Option<LobbyWin> {
        self.lobby_window_full(name, w, h, off, sel, hand, hide, swaps, [0.0; 4])
    }

    /// Ours, not retail: the frame grown by `pad` (left, top, right, bottom) around the unchanged content; its left
    /// and bottom edges stay where the layout puts them (the content moves right and up).
    pub fn lobby_window_padded(&mut self, name: &str, w: f32, h: f32, sel: Option<u16>, hand: bool, pad: [f32; 4]) -> Option<LobbyWin> {
        self.lobby_window_full(name, w, h, (pad[0], -pad[3]), sel, hand, &[], &[], pad)
    }

    #[allow(clippy::too_many_arguments)]
    fn lobby_window_full(&mut self, name: &str, w: f32, h: f32, off: (f32, f32), sel: Option<u16>, hand: bool, hide: &[u16], swaps: &[(u16, Option<&str>, usize)], pad: [f32; 4]) -> Option<LobbyWin> {
        let frame = self.cursor_frame();
        let (dx, dy) = (off.0, off.1 + self.stack_dy(name, w, h));
        let grow = |(fx, fy, fw, fh): (f32, f32, f32, f32)| (fx - pad[0], fy - pad[1], fw + pad[0] + pad[2], fh + pad[1] + pad[3]);
        let (fx, fy, fw, fh) = grow(screens::frame_rect(self.data.lobby.menu(name)?, w, h, 1.0, 1.0));
        if !self.win_gate(name, [fx + dx, fy + dy, fw, fh]) {
            return None;
        }
        let ui = &self.data.lobby;
        let m = ui.menu(name)?;
        let (x, y) = screens::window_pos(m, w, h, 1.0, 1.0);
        let (fx, fy, fw, fh) = grow(screens::frame_rect(m, w, h, 1.0, 1.0));
        let (x, y, fx, fy) = (x + dx, y + dy, fx + dx, fy + dy);
        self.batch.push_frame(ui, &self.data.style, fx, fy, fw, fh, 1.0, 1.0, 0x8080_8080, 0, false, false);
        if let Some(r) = m.elements[0].refs.iter().find(|r| r.kind == 0)
            && let Some(lib) = ui.lib(&r.lib)
        {
            self.batch.push_sprite_from(ui, lib, r.index as usize, 1, x, y, 1.0, 1.0, 0x8080_8080, 0);
        }
        let mut children = Vec::new();
        let pressed = self.presses.borrow().get(name).copied();
        for c in m.elements.iter().skip(1).filter(|c| !hide.contains(&c.id)) {
            let (cx, cy) = (x + c.x as f32, y + c.y as f32);
            if hand && sel == Some(c.id) {
                self.focus.0 = Some((name.to_string(), c.id, [cx, cy, c.w as f32, c.h as f32]));
            }
            if let Some(r) = c.refs.iter().find(|r| r.kind == 0)
                && let Some(own) = ui.lib(&r.lib)
            {
                let on = sel == Some(c.id);
                let press = pressed.filter(|p| on && p.0 == c.id).map(|p| p.1);
                let d = if press.is_some() { 1.0 } else if on { -1.0 } else { 0.0 };
                let (lib, k) = match swaps.iter().find(|s| s.0 == c.id) {
                    Some(&(_, Some(l), k)) => (ui.lib(l).unwrap_or(own), k),
                    Some(&(_, None, k)) => (own, k),
                    None => (own, r.index as usize),
                };
                self.batch.push_sprite(ui, lib, k, cx + d, cy + d, 1.0, 1.0, if on { 0x8080_5020 } else { 0x8080_8080 }, 0);
                if let Some(t) = press {
                    let (bw, bh) = (screens::sprite_width(lib, k), screens::sprite_height(lib, k));
                    for (s, a, blend) in screens::press_rings(20.0, t) {
                        let (rx, ry) = (cx + d - ((s - 1.0) * bw / 2.0).trunc(), cy + d - ((s - 1.0) * bh / 2.0).trunc());
                        self.batch.push_sprite(ui, lib, k, rx, ry, s, s, a << 24 | 0x80_5020, blend);
                    }
                }
                if on
                    && hand
                    && let Some(anc) = ui.lib("anc     anc_s")
                {
                    self.batch.push_sprite(ui, anc, frame % anc.sprites.len().max(1), cx, cy, 1.0, 1.0, 0x8080_8080, 0);
                }
            }
            children.push((c.id, [cx, cy, c.w as f32, c.h as f32]));
        }
        Some(LobbyWin { frame: [fx, fy, fw, fh], children })
    }

    /// FUN_1011a1a0 in (x0, y0, x1, y1): a block of lines 16 px apart, centred when it fits, else from the corner
    /// with lines wrapped at spaces; lines are left-aligned in the block.
    pub fn text_block(&mut self, text: &str, r: [f32; 4]) {
        let (bw, bh) = (r[2] - r[0], r[3] - r[1]);
        let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        let widest = |l: &[String], g: &GameUi| l.iter().map(|s| g.text_width(s)).fold(0.0, f32::max);
        let fits = widest(&lines, self) <= bw;
        if !fits {
            lines = lines.iter().flat_map(|l| self.wrap(l, bw)).collect();
        }
        let th = lines.len() as f32 * 16.0;
        let x = if fits { r[0] + ((bw - widest(&lines, self)) / 2.0).trunc() } else { r[0] };
        let y = if th <= bh { r[1] + ((bh - th) / 2.0).trunc() } else { r[1] };
        for (i, l) in lines.iter().enumerate() {
            self.text(l, x, y + 16.0 * i as f32, 0, WHITE);
        }
    }

    /// FUN_10231750: a text rect kept 16 px inside the UI area.
    fn clamp_rect(f: [f32; 4], w: f32, h: f32) -> [f32; 4] {
        [f[0].max(16.0), f[1].max(16.0), (f[0] + f[2]).min(w - 16.0), (f[1] + f[3]).min(h - 16.0)]
    }

    /// ptc6yesn (draw FUN_10231f50), centred: the text in the frame rect with x0 + 0x30 (all but Japanese) and 28 px
    /// off the bottom, Yes / No (children 1 / 2), the warning icon (child 3) unless hidden. Returns the Yes / No rects.
    pub fn yes_no_window(&mut self, text: &str, yes: bool, icon: bool, w: f32, h: f32) -> Option<Vec<(u16, [f32; 4])>> {
        let hide: &[u16] = if icon { &[] } else { &[3] };
        let win = self.lobby_window("menu    ptc6yesn", w, h, true, Some(if yes { 1 } else { 2 }), true, hide)?;
        let r = Self::clamp_rect(win.frame, w, h);
        self.text_block(text, [r[0] + 48.0, r[1], r[2], r[3] - 28.0]);
        Some(win.children.into_iter().filter(|(id, _)| *id <= 2).collect())
    }

    /// menu ok (FUN_102319a0), centred: the text in the frame rect, 28 px off the bottom, and its OK child.
    pub fn ok_window(&mut self, text: &str, w: f32, h: f32) -> Option<[f32; 4]> {
        let win = self.lobby_window("menu    ok", w, h, true, Some(1), true, &[])?;
        let r = Self::clamp_rect(win.frame, w, h);
        self.text_block(text, [r[0], r[1], r[2], r[3] - 28.0]);
        win.children.first().map(|c| c.1)
    }

    /// What an FFXI error code shows (FUN_10017310 / FUN_10017380): "Error code: FFXI-%04d" (group 7 id 1), then for
    /// the table's kind-2 codes a line break and their message.
    pub fn error_text(&self, code: u32) -> String {
        let head = super::select::printf(&self.lobby_msg(7, 1, ""), &[&code.to_string()]);
        match self.data.errors.iter().find(|r| r[0] == code) {
            Some(&[_, 2, g, id]) => format!("{head}\n{}", self.lobby_msg(g as u8, id as usize, "")),
            _ => head,
        }
    }

    /// The info panel's area: group 4's short name (FUN_102333b0, "unknown" when the entry is missing, as retail).
    /// FUN_101cbc10(zone, 0): the group 4 short name, none when the table has no entry.
    pub fn short_area(&self, zone: u16) -> Option<String> {
        let raw = self.data.area_names.as_ref().and_then(|m| m.raw(zone as usize))?;
        Some(crate::dat::font::decode_sjis(&raw)).filter(|s| !s.is_empty())
    }

    pub fn area_name(&self, zone: u16) -> Vec<u8> {
        match self.data.area_names.as_ref().and_then(|m| m.raw(zone as usize)) {
            Some(n) => n,
            None => {
                log::error!("no group 4 area name for zone {zone}");
                b"unknown".to_vec()
            }
        }
    }

    pub fn count_formats(&self) -> [String; 4] {
        self.data.count_fmt.clone()
    }

    /// The help bar alone (lobby screens over a 3D scene); returns its text slot.
    pub fn help_frame(&mut self, w: f32, h: f32) -> Option<TextSlot> {
        screens::push_help_frame(&mut self.batch, &self.data.lobby, &self.data.style, w, h)
    }

    /// A lobby menu window (creator steps); returns the label rects.
    /// A window's layout (menu DAT 39542 and its companions).
    pub fn layout(&self, name: &str) -> Option<&crate::dat::ui::Menu> {
        self.data.lobby.menu(name)
    }

    pub fn menu(&mut self, name: &str, w: f32, h: f32, sel: usize) -> Vec<[f32; 4]> {
        screens::push_menu(&mut self.batch, &self.data.lobby, &self.data.style, name, w, h, sel)
    }

    /// A HUD window by menu name (frame and its first sprite); returns its position.
    pub fn window(&mut self, name: &str, w: f32, h: f32) -> Option<(f32, f32)> {
        let dy = self.stack_dy(name, w, h);
        if dy != 0.0 {
            return self.window_off_grow(name, w, h, 0.0, 0.0, 0.0);
        }
        let m = self.data.lobby.menu(name)?;
        let (fx, fy, fw, fh) = screens::frame_rect(m, w, h, 1.0, 1.0);
        if !self.win_gate(name, [fx, fy, fw, fh]) {
            return None;
        }
        let m = self.data.lobby.menu(name)?;
        Some(screens::push_window(&mut self.batch, &self.data.lobby, &self.data.style, m, w, h))
    }

    /// Sprite `k` of library `lib` at (x, y), ARGB tint with 0x80 = 1.0.
    pub fn sprite(&mut self, lib: &str, k: usize, x: f32, y: f32, tint: u32) -> bool {
        let Some(l) = self.data.lobby.lib(lib) else { return false };
        if k >= l.sprites.len() {
            return false;
        }
        self.batch.push_sprite(&self.data.lobby, l, k, x, y, 1.0, 1.0, tint, 0);
        true
    }

    /// Sprite `k` scaled by (sx, sy) about (x, y).
    #[allow(clippy::too_many_arguments)]
    pub fn sprite_scaled(&mut self, lib: &str, k: usize, x: f32, y: f32, sx: f32, sy: f32, tint: u32) {
        if let Some(l) = self.data.lobby.lib(lib).filter(|l| k < l.sprites.len()) {
            self.batch.push_sprite(&self.data.lobby, l, k, x, y, sx, sy, tint, 0);
        }
    }

    /// The open / close animation for a window a screens-level drawer places itself (FUN_101177d0): false while it
    /// opens (only the zooming frame draws); stopping drawing it plays its close.
    pub fn gate_layout(&mut self, name: &str, w: f32, h: f32, x: Option<i16>) -> bool {
        self.gate_layout_at(name, w, h, x, None)
    }

    /// `gate_layout` with the layout place's x and / or y replaced (a window its open hook moves).
    pub fn gate_layout_at(&mut self, name: &str, w: f32, h: f32, x: Option<i16>, y: Option<i16>) -> bool {
        let Some(mut m) = self.stacked_menu(name, w, h) else { return true };
        if let Some(e) = m.elements.first_mut() {
            e.x = x.unwrap_or(e.x);
            e.y = y.unwrap_or(e.y);
        }
        let (fx, fy, fw, fh) = screens::frame_rect(&m, w, h, 1.0, 1.0);
        self.win_gate(name, [fx, fy, fw, fh])
    }

    /// A window that swaps its contents in place (an ability type list for its parent, an emote folder) is a new
    /// window to retail: its open plays again when `key` changes.
    pub fn replay_on_change(&mut self, name: &str, key: u64) {
        let prev = self.contents.insert(name.to_string(), key);
        if prev.is_some_and(|p| p != key)
            && let Some(a) = self.anims.get_mut(name)
        {
            a.seen = 0;
        }
    }

    /// As gate_layout for a frame the caller draws itself.
    pub fn gate_rect(&mut self, name: &str, r: [f32; 4]) -> bool {
        self.win_gate(name, r)
    }

    /// A layout moved by its region stacking, for the screens-level drawers that place windows themselves.
    pub fn stacked_menu(&self, name: &str, w: f32, h: f32) -> Option<crate::dat::ui::Menu> {
        let dy = self.stack_dy(name, w, h);
        let mut m = self.data.lobby.menu(name)?.clone();
        if let Some(e) = m.elements.first_mut() {
            e.y = (e.y as f32 + dy).round() as i16;
        }
        Some(m)
    }

    /// Sprite `k` scaled by (sx, sy) with a blend word (8: the gauge fills).
    #[allow(clippy::too_many_arguments)]
    pub fn sprite_scaled_blend(&mut self, lib: &str, k: usize, x: f32, y: f32, sx: f32, sy: f32, tint: u32, blend: u32) {
        if let Some(l) = self.data.lobby.lib(lib).filter(|l| k < l.sprites.len()) {
            self.batch.push_sprite(&self.data.lobby, l, k, x, y, sx, sy, tint, blend);
        }
    }

    /// Sprite `k` scaled, its cells' vertex colours replaced by `rgba` (0x80 = 1.0).
    #[allow(clippy::too_many_arguments)]
    pub fn sprite_coloured(&mut self, lib: &str, k: usize, x: f32, y: f32, sx: f32, sy: f32, rgba: [u8; 4], blend: u32) {
        let Some(sp) = self.data.lobby.lib(lib).and_then(|l| l.sprites.get(k)) else { return };
        for c in &sp.cells {
            let c = crate::dat::ui::Cell { colors: [rgba; 4], ..c.clone() };
            self.batch.push_cell(&self.data.lobby, &c, x, y, sx, sy, 0x8080_8080, blend);
        }
    }

    /// Sprite `k` through a 2x2 matrix about (x, y) (the compass disk).
    pub fn sprite_affine(&mut self, lib: &str, k: usize, x: f32, y: f32, m: [f32; 4], tint: u32) {
        self.sprite_affine_blend(lib, k, x, y, m, tint, 0);
    }

    /// The same with a blend word (0: the cells' own; 0x44 alpha blended).
    #[allow(clippy::too_many_arguments)]
    pub fn sprite_affine_blend(&mut self, lib: &str, k: usize, x: f32, y: f32, m: [f32; 4], tint: u32, blend: u32) {
        let Some(l) = self.data.lobby.lib(lib) else { return };
        if let Some(s) = l.sprites.get(k) {
            for c in &s.cells {
                self.batch.push_cell_affine_blend(&self.data.lobby, c, x, y, m, tint, blend);
            }
        }
    }

    /// The right edge (max x) of sprite `k`, in sprite space.
    pub fn sprite_right(&self, lib: &str, k: usize) -> f32 {
        let Some(s) = self.data.lobby.lib(lib).and_then(|l| l.sprites.get(k)) else { return 0.0 };
        s.cells.iter().flat_map(|c| c.corners.iter().map(|p| p[0])).max().unwrap_or(0) as f32
    }

    pub fn sprite_width(&self, lib: &str, k: usize) -> f32 {
        self.data.lobby.lib(lib).map_or(0.0, |l| screens::sprite_width(l, k))
    }

    /// Sprite `k` from cell `skip` on (window sprites, whose first cell is the background).
    pub fn sprite_from(&mut self, lib: &str, k: usize, skip: usize, x: f32, y: f32, tint: u32) {
        if let Some(l) = self.data.lobby.lib(lib) {
            self.batch.push_sprite_from(&self.data.lobby, l, k, skip, x, y, 1.0, 1.0, tint, 0);
        }
    }

    /// Window frame style `k` (config 0x12: file 14 + k, FUN_10159a70): its pieces and textures.
    pub fn set_style(&mut self, k: usize) {
        if k == self.style_index || k >= self.data.styles.len() {
            return;
        }
        let f = &self.data.styles[k];
        let Some(lib) = f.libs.first() else { return };
        self.style_index = k;
        self.data.style = lib.clone();
        for (name, img) in &f.images {
            let old = self.data.lobby.images.insert(name.clone(), crate::dat::ui::UiImage { width: img.width, height: img.height, rgba: img.rgba.clone() });
            if let Some(old) = old
                && !self.style_saved.contains_key(name)
            {
                self.style_saved.insert(name.clone(), old);
            }
            self.replaced.push((name.clone(), img.width, img.height, img.rgba.clone()));
        }
    }

    /// Back to the lobby: the startup images return. Retail's lobby draws from its own menu files, which no style
    /// touches; ours share one image table with the game's windows.
    pub fn reset_style(&mut self) {
        for (name, img) in self.style_saved.drain() {
            self.replaced.push((name.clone(), img.width, img.height, img.rgba.clone()));
            self.data.lobby.images.insert(name, img);
        }
        self.style_index = 0;
        if let Some(lib) = self.data.styles.first().and_then(|f| f.libs.first()) {
            self.data.style = lib.clone();
        }
    }

    /// Cells `cells` of sprite `k` at (x, y) (a button's blank cells without its caption).
    pub fn sprite_cells(&mut self, lib: &str, k: usize, cells: std::ops::Range<usize>, x: f32, y: f32, tint: u32) {
        let Some(s) = self.data.lobby.lib(lib).and_then(|l| l.sprites.get(k)) else { return };
        for c in s.cells.get(cells).unwrap_or_default() {
            self.batch.push_cell(&self.data.lobby, c, x, y, 1.0, 1.0, tint, 0);
        }
    }

    /// A list's scroll bar (screens::push_scroll_bar).
    pub fn scroll_bar(&mut self, x: f32, y: f32, h: f32, total: usize, visible: usize, top: usize) {
        screens::push_scroll_bar(&mut self.batch, &self.data.lobby, x, y, h, total, visible, top);
    }

    /// A focused list's bar at (x, y), h tall, for the mouse and the wheel; registered whether it shows or not (the
    /// wheel goes to the focused list either way).
    #[allow(clippy::too_many_arguments)]
    pub fn bar_site(&mut self, owner: super::scrollbar::Owner, x: f32, y: f32, h: f32, total: usize, visible: usize, top: usize) {
        self.bars.push(super::scrollbar::Site { owner, rect: [x, y, h], bar: (total, visible, top) });
    }

    /// A focused list takes the wheel (its bar drawn last frame).
    pub fn wants_wheel(&self) -> bool {
        !self.bars.is_empty()
    }

    /// Cell `cell` of sprite `k` stretched over (x, y, w, h), keeping its texture rect.
    #[allow(clippy::too_many_arguments)]
    pub fn cell_stretch(&mut self, lib: &str, k: usize, cell: usize, x: f32, y: f32, w: f32, h: f32, tint: u32) {
        self.cell_stretch_blend(lib, k, cell, x, y, w, h, tint, 0);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn cell_stretch_blend(&mut self, lib: &str, k: usize, cell: usize, x: f32, y: f32, w: f32, h: f32, tint: u32, blend_word: u32) {
        let Some(c) = self.data.lobby.lib(lib).and_then(|l| l.sprites.get(k)).and_then(|s| s.cells.get(cell)) else { return };
        let (cw, ch) = (c.size[0] as f32, c.size[1] as f32);
        self.batch.push_cell_rect(&self.data.lobby, c, x, y, cw, ch, w / cw.max(1.0), h / ch.max(1.0), tint, blend_word);
    }

    /// A menu layout by name (window element first, then the children).
    pub fn menu_def(&self, name: &str) -> Option<&crate::dat::ui::Menu> {
        self.data.lobby.menu(name)
    }

    /// A HUD window moved by (dx, dy): frame and window sprite; returns its position.
    pub fn window_off(&mut self, name: &str, w: f32, h: f32, dx: f32, dy: f32) -> Option<(f32, f32)> {
        self.window_off_grow(name, w, h, dx, dy, 0.0)
    }

    /// As window_off, the frame `grow` px taller (rows of ours under a list).
    pub fn window_off_grow(&mut self, name: &str, w: f32, h: f32, dx: f32, dy: f32, grow: f32) -> Option<(f32, f32)> {
        let dy = dy + self.stack_dy(name, w, h);
        let m = self.data.lobby.menu(name)?;
        let (x, y) = screens::window_pos(m, w, h, 1.0, 1.0);
        let (fx, fy, fw, fh) = screens::frame_rect(m, w, h, 1.0, 1.0);
        let fh = fh + grow;
        if !self.win_gate(name, [fx + dx, fy + dy, fw, fh]) {
            return None;
        }
        let m = self.data.lobby.menu(name)?;
        self.batch.push_frame(&self.data.lobby, &self.data.style, fx + dx, fy + dy, fw, fh, 1.0, 1.0, 0x8080_8080, 0, false, false);
        if let Some(r) = m.elements[0].refs.iter().find(|r| r.kind == 0)
            && let Some(lib) = self.data.lobby.lib(&r.lib)
        {
            self.batch.push_sprite_from(&self.data.lobby, lib, r.index as usize, 1, x + dx, y + dy, 1.0, 1.0, 0x8080_8080, 0);
        }
        Some((x + dx, y + dy))
    }

    /// A layout window's position (FUN_101160d0); None once its gate ran this frame while it still opens, so its
    /// contents wait for the zoom.
    pub fn window_xy(&self, name: &str, w: f32, h: f32) -> Option<(f32, f32)> {
        if self.anims.get(name).is_some_and(|a| a.seen == self.anim_frame && a.open > 0.0) {
            return None;
        }
        let (x, y) = screens::window_pos(self.data.lobby.menu(name)?, w, h, 1.0, 1.0);
        Some((x, y + self.stack_dy(name, w, h)))
    }

    /// A message of group 0xb (key assignment labels and prompts).
    pub fn group_b(&self, id: usize) -> Option<String> {
        self.data.lobby_msgs.get(&0x0b).and_then(|m| m.get(id)).filter(|s| !s.is_empty())
    }

    /// A cell's corners (x0, y0, x1, y1) in sprite space.
    pub fn cell_bounds(&self, lib: &str, k: usize, cell: usize) -> Option<[f32; 4]> {
        let c = self.data.lobby.lib(lib)?.sprites.get(k)?.cells.get(cell)?;
        Some([c.corners[0][0] as f32, c.corners[0][1] as f32, c.corners[3][0] as f32, c.corners[3][1] as f32])
    }

    /// Frames in a sprite library (animated cursors such as "anc     kaipage").
    pub fn lib_len(&self, lib: &str) -> usize {
        self.data.lobby.lib(lib).map_or(0, |l| l.sprites.len())
    }

    /// Uploads an RGBA image under `name` once (item icons).
    pub fn image(&mut self, name: &str, w: u32, h: u32, rgba: Vec<u8>) {
        if self.images.insert(name.to_string()) {
            self.pending.push((name.to_string(), w, h, rgba));
        }
    }

    pub fn has_image(&self, name: &str) -> bool {
        self.images.contains(name)
    }

    /// The batch with the menu file, window style and font, for the window builders in render/invwin.rs.
    pub fn parts(&mut self) -> (&mut UiBatch, &UiFile, &SpriteLib, &Font) {
        (&mut self.batch, &self.data.lobby, &self.data.style, &self.data.font)
    }

    pub const ATLAS: &'static str = ATLAS;

    /// Renders this frame's batch headless to a PNG (offscreen shots).
    pub fn save_png(&self, width: u32, height: u32, clear: [f64; 3], out: &std::path::Path) -> Result<()> {
        use crate::dat::ui::{UiImage, norm_name};
        let mut fonts = UiFile::default();
        let a = &self.data.font.atlas;
        fonts.images.insert(ATLAS.into(), UiImage { width: a.width, height: a.height, rgba: a.rgba.clone() });
        for (name, t) in &self.data.font.textures {
            fonts.images.insert(norm_name(name.as_bytes()), UiImage { width: t.width, height: t.height, rgba: t.rgba.clone() });
        }
        for (name, w, h, rgba) in &self.pending {
            fonts.images.insert(name.clone(), UiImage { width: *w, height: *h, rgba: rgba.clone() });
        }
        crate::render::ui::render_png(&[&self.data.lobby, &fonts], &self.batch, width, height, clear, out)
    }

    pub fn vertex_count(&self) -> usize {
        self.batch.vertex_count()
    }

    /// Text in a box: centred when it fits, else scrolled left as the help line does (FUN_10236b40: 0x1e frames'
    /// wait, then 2 px a frame at 30 frames a second, looping), clipped to the box.
    pub fn scroll_text(&mut self, text: &str, r: [f32; 4], color: [u8; 4]) {
        let width = self.text_width(text);
        if width <= r[2] - 8.0 {
            self.text(text, r[0] + r[2] / 2.0, r[1], 1, color);
            return;
        }
        let since = *self.scroll_seen.entry(text.to_string()).or_insert_with(Instant::now);
        let frames = since.elapsed().as_secs_f32() * 30.0;
        // The gap goes after the text, so it starts readable at the left edge.
        let looped = format!("{text}\u{3000}\u{3000}\u{3000}\u{3000}");
        let cycle = self.text_width(&looped);
        let off = if frames < 30.0 { 0.0 } else { ((frames - 30.0) * 2.0).trunc() % cycle };
        self.clip(Some(r));
        self.text(&looped, r[0] + 4.0 - off, r[1], 0, color);
        if cycle - off < r[2] {
            self.text(&looped, r[0] + 4.0 + cycle - off, r[1], 0, color);
        }
        self.clip(None);
    }

    /// Clips what is pushed next to (x, y, w, h); None ends it.
    pub fn clip(&mut self, clip: Option<[f32; 4]>) {
        self.batch.set_clip(clip);
    }

    /// Quads drawn from now on are depth-tested against the scene at NDC z `depth` (name plates); None = not.
    pub fn depth(&mut self, depth: Option<f32>) {
        self.batch.set_depth(depth);
    }

    /// Starts an empty frame (the HUD frame: name plates, then the windows).
    pub fn clear(&mut self) {
        self.batch = UiBatch::default();
        self.log_top = None;
        self.target_up = false;
    }

    /// A window's registry flags; the action menu's layouts (actionm1 .. actiom10) count as "menu    playermo"
    /// (FUN_1011a8f0).
    fn reg_flags(&self, name: &str) -> u32 {
        let key = name.trim_end().to_ascii_lowercase();
        let reg = if key.starts_with("menu    actionm") || key.starts_with("menu    actiom") { "menu    playermo" } else { key.as_str() };
        self.data.registry.get(reg).copied().unwrap_or(0) | self.walk.flags.get(&key).copied().unwrap_or(0)
    }

    /// Window flags a window set on itself while open (the comment editor's 0x1000, FUN_101dd2b0); None clears them.
    pub fn force_flags(&mut self, name: &str, f: Option<u32>) {
        let key = name.trim_end().to_ascii_lowercase();
        match f {
            Some(f) => self.walk.flags.insert(key, f),
            None => self.walk.flags.remove(&key),
        };
    }

    /// FUN_1015ecc0(mask): a window with registry flags & mask drawn this or last frame.
    pub fn open_with_flag(&self, mask: u32) -> bool {
        self.anims.iter().any(|(n, a)| a.seen + 1 >= self.anim_frame && self.reg_flags(n) & mask != 0)
    }

    /// The frames of those windows as last drawn.
    pub fn frames_with_flag(&self, mask: u32) -> Vec<[f32; 4]> {
        self.anims.iter().filter(|(n, a)| a.seen + 1 >= self.anim_frame && self.reg_flags(n) & mask != 0).filter_map(|(_, a)| if let Shape::Frame(r) = &a.shape { Some(*r) } else { None }).collect()
    }

    /// The left edge of the windows drawn last frame that reach the screen's right edge (within 32) over rows [y0, y1).
    pub fn edge_column(&self, sw: f32, y0: f32, y1: f32) -> Option<f32> {
        let frames = self.anims.values().filter(|a| a.seen + 1 >= self.anim_frame).filter_map(|a| if let Shape::Frame(r) = &a.shape { Some(*r) } else { None });
        frames.filter(|r| r[0] + r[2] >= sw - 32.0 && r[1] < y1 && r[1] + r[3] > y0).map(|r| r[0]).reduce(f32::min)
    }

    /// A window's registry group mask (FUN_1015ed10's +0x38), or the one its open hook set.
    pub fn win_group(&self, name: &str) -> u16 {
        let key = name.trim_end().to_ascii_lowercase();
        self.walk.groups.get(&key).or_else(|| self.data.groups.get(key.as_str())).copied().unwrap_or(0)
    }

    /// A window's group byte changed while open (persona in Status mode: |= 0x20, FUN_10146ec0); None restores it.
    pub fn set_group(&mut self, name: &str, g: Option<u16>) {
        let key = name.trim_end().to_ascii_lowercase();
        match g {
            Some(g) => self.walk.groups.insert(key, g),
            None => self.walk.groups.remove(&key),
        };
    }

    /// Region stacking (FUN_1015fa00 -> FUN_1015f790): registry flag 0x1000 windows sit on the chat log (window rect
    /// top = log top - height - 2, at least 16; the frame keeps its header offset from it), flag 0x2000 ones on the
    /// party window. The y offset from the layout.
    pub fn stack_dy(&self, name: &str, w: f32, h: f32) -> f32 {
        let flags = self.reg_flags(name);
        match (self.log_top, self.data.lobby.menu(name)) {
            (Some(top), Some(m)) if flags & 0x1000 != 0 => {
                let (_, wy) = screens::window_pos(m, w, h, 1.0, 1.0);
                let wh = m.elements[0].h as f32;
                // On the log's bottom (h - 16) while it shows no lines, else 2 px above its top.
                let y = if self.log_faded { h - 16.0 - wh } else { top - wh - 2.0 };
                y.max(16.0) - wy
            }
            // Region 2 (flag 0x4000, subwindo): on the target window, else on the party window (FUN_1015e530).
            (_, Some(m)) if flags & 0x4000 != 0 => {
                let (_, wy) = screens::window_pos(m, w, h, 1.0, 1.0);
                let wh = m.elements[0].h as f32;
                let base = if self.target_up { self.window_rect("menu    targetwi", w, h) } else { self.window_rect(&self.party_window, w, h) };
                base.map_or(0.0, |r| (r[1] - wh - 2.0).max(16.0) - wy)
            }
            // Region 1 (flag 0x2000: targetwi, the region map legends): on the party window (FUN_10158e10).
            (_, Some(m)) if flags & 0x2000 != 0 => {
                let (_, wy) = screens::window_pos(m, w, h, 1.0, 1.0);
                let wh = m.elements[0].h as f32;
                self.window_rect(&self.party_window, w, h).map_or(0.0, |p| (p[1] - wh - 2.0).max(16.0) - wy)
            }
            _ => screens::stacked_dy(&self.data.lobby, name, w, h, &self.party_window),
        }
    }

    /// The HUD windows (chat log, party, target, main menu), after what is already in the frame.
    pub fn hud(&mut self, w: f32, h: f32, st: &HudState) -> Vec<TextSlot> {
        self.party_window = st.party_window.clone();
        self.target_up = st.target.is_some();
        // raid1 / raid2 open with the zoom (registry flags 3, no 8): nothing of them until it is done.
        let mut st = st.clone();
        for (k, name) in ["menu    raid1", "menu    raid2"].iter().enumerate() {
            if st.alliance[k].is_some()
                && let Some(r) = self.window_rect(name, w, h)
                && !self.win_gate(name, r)
            {
                st.alliance[k] = None;
            }
        }
        // The main menu zooms open and closed like any window; a page swap closes one and opens the other
        // (FUN_10140400 messages 3 / 4: socialme opened, menuwind closed). Nothing of it while it opens.
        let page = screens::menu_window(st.menu_page);
        if st.menu_selected.is_some()
            && let Some(m) = self.data.lobby.menu(page)
        {
            let (fx, fy, fw, fh) = screens::frame_rect(m, w, h, 1.0, 1.0);
            if !self.win_gate(page, [fx, fy, fw, fh]) {
                st.menu_selected = None;
            }
        }
        let (b, text) = screens::hud_screen(&self.data.lobby, &self.data.style, w, h, &st);
        self.batch.append(b);
        text
    }

    /// The party / alliance window hand (screens::push_party_hand), after the chat log.
    pub fn party_hand(&mut self, w: f32, h: f32, st: &HudState) {
        screens::push_party_hand(&mut self.batch, &self.data.lobby, st, w, h);
    }

    /// A name plate in a name colour (an "menu    ncol" tint, FUN_10119590), bottom at (x, y).
    /// A name plate: the icon bytes (client/plate.rs) and the name, the job master stars above it.
    #[allow(clippy::too_many_arguments)]
    pub fn name_plate(&mut self, prefix: &[u8], name: &str, x: f32, y: f32, scale: [f32; 2], colour: u32, pulse: f32, icon: u32, stars: bool) {
        // Alpha is MODULATE4X in that pass: a full-alpha tint saturates it as the card does; `pulse` scales it.
        let a = (255.0 * pulse.clamp(0.0, 1.0)) as u32;
        let tint = colour & 0x00ff_ffff | a << 24;
        screens::push_name_plate(&mut self.batch, &self.data.lobby, prefix, &encode_sjis(name), x, y, scale[0], scale[1], tint, icon);
        if stars {
            screens::push_plate_stars(&mut self.batch, &self.data.lobby, x, y, scale[0], scale[1]);
        }
    }

    /// Sprite-font text ("font    fontshp"), left aligned or ending at x.
    pub fn shape_text(&mut self, s: &str, x: f32, y: f32, tint: u32, right: bool) -> f32 {
        let b = encode_sjis(s);
        if right { screens::push_shape_text_right(&mut self.batch, &self.data.lobby, &b, x, y, tint) } else { screens::push_shape_text(&mut self.batch, &self.data.lobby, &b, x, y, tint) }
    }

    /// A Config page caption in fontshp (see screens::push_label); returns the end x.
    pub fn label(&mut self, s: &str, x: f32, y: f32, tint: u32, shadow: bool) -> f32 {
        screens::push_label(&mut self.batch, &self.data.lobby, s, x, y, tint, shadow)
    }

    pub fn label_width(&self, s: &str) -> f32 {
        screens::label_width(s)
    }

    /// A sprite without its caption: the cells not taken from the font texture (a button's blank body).
    pub fn sprite_base(&mut self, lib: &str, k: usize, x: f32, y: f32, tint: u32, blend_word: u32) {
        let Some(s) = self.data.lobby.lib(lib).and_then(|l| l.sprites.get(k)) else { return };
        for c in s.cells.iter().filter(|c| !c.texture.starts_with("font")) {
            self.batch.push_cell(&self.data.lobby, c, x, y, 1.0, 1.0, tint, blend_word);
        }
    }

    /// FUN_101c9fa0: a name wrapped as the target window draws it (see screens::push_wrapped_name).
    #[allow(clippy::too_many_arguments)]
    pub fn wrapped_name(&mut self, s: &str, max: f32, x: f32, y: f32, tint: u32, centre: bool, split: bool) {
        screens::push_wrapped_name(&mut self.batch, &self.data.lobby, &encode_sjis(s), max, x, y, tint, centre, split);
    }

    /// The name-plate colour for an ncol entry ("menu    ncol").
    pub fn ncol(&self, ncol: usize) -> u32 {
        screens::sprite_tint(&self.data.lobby, "menu    ncol", ncol)
    }

    pub fn shape_name(&mut self, s: &str, x: f32, y: f32, tint: u32) {
        screens::push_shape_name(&mut self.batch, &self.data.lobby, &encode_sjis(s), x, y, tint);
    }

    pub fn hp_tint(&self, hpp: u8) -> u32 {
        screens::hp_tint(&self.data.lobby, hpp)
    }

    pub fn menu_items(&self, w: f32, h: f32, page: u8) -> Vec<(usize, [f32; 4])> {
        screens::hud_menu_items(&self.data.lobby, w, h, page)
    }

    pub fn menu_arrows(&self, w: f32, h: f32, page: u8) -> Vec<[f32; 4]> {
        screens::hud_menu_arrows(&self.data.lobby, w, h, page)
    }

    /// A message of group 0xe, or `fallback` when the file did not load.
    pub fn menu_msg(&self, id: usize, fallback: &str) -> String {
        self.data.menu_msgs.as_ref().and_then(|m| m.get(id)).unwrap_or_else(|| fallback.to_string())
    }

    /// A message of group 0xd (window titles); None when missing or empty.
    pub fn title_msg(&self, id: usize) -> Option<String> {
        self.data.title_msgs.as_ref().and_then(|m| m.get(id)).filter(|s| !s.is_empty())
    }

    /// The action menu's help line for a command: group 0xe by the table at 0x103634fc (FUN_10127790).
    pub fn cmd_help(&self, cmd: u8) -> Option<String> {
        let id = *self.data.cmd_help.get(cmd as usize)?;
        self.data.menu_msgs.as_ref().and_then(|m| m.get(id as usize)).filter(|s| !s.is_empty())
    }

    /// What a focused window puts in the bars' slot 1 (FUN_10115380): the child's first string (group 0xe) as
    /// help and its second (group 0xd) as title, each falling back to the window header's (FUN_101152c0 / FUN_10115320).
    /// A layout child's own sprite (FUN_10118ad0: its state-0 reference), as (library, index).
    pub fn child_sprite(&self, name: &str, child: u16) -> Option<(String, usize)> {
        let m = self.data.lobby.menu(name)?;
        let r = m.elements.iter().skip(1).find(|e| e.id == child)?.refs.iter().find(|r| r.kind == 0)?;
        Some((r.lib.clone(), r.index as usize))
    }

    pub fn focus_text(&self, name: &str, child: u16) -> [Option<String>; 2] {
        let Some(m) = self.data.lobby.menu(name) else { return [None, None] };
        let msg = |e: &crate::dat::ui::MenuElement, k: usize| {
            let id = e.strings[k].parse::<i32>().ok().filter(|&i| i >= 0)? as usize;
            if k == 0 { self.data.menu_msgs.as_ref()?.get(id).filter(|s| !s.is_empty()) } else { self.title_msg(id) }
        };
        let item = m.elements.iter().skip(1).find(|e| e.id == child);
        [0, 1].map(|k| item.and_then(|e| msg(e, k)).or_else(|| msg(&m.elements[0], k)))
    }

    pub fn map_regions(&self) -> &[super::map::Region] {
        &self.data.map_regions
    }

    pub fn region_groups(&self) -> &[(u8, Vec<u16>)] {
        &self.data.region_groups
    }

    /// The party window in use (ptw0..2 by the member count).
    pub fn party_window_name(&self) -> &str {
        &self.party_window
    }

    /// Scaled, centred fontshp text (FUN_101c97b0).
    pub fn shape_scaled_centred(&mut self, text: &[u8], x: f32, y: f32, s: f32, tint: u32) {
        screens::push_shape_scaled_centred(&mut self.batch, &self.data.lobby, text, x, y, s, tint);
    }

    /// A status icon's texture name, uploaded on first use (FUN_102272b0 reads it; nothing draws before).
    pub fn status_icon(&mut self, small: bool, id: u16) -> Option<String> {
        let name = format!("status icon {}:{id}", small as u8);
        if !self.has_image(&name) {
            let (w, h, rgba) = crate::dat::statusinfo::icon(self.data.status[small as usize].as_deref()?, id)?;
            self.image(&name, w, h, rgba);
        }
        Some(name)
    }

    /// A status icon's pixels (the modern layer's copy).
    /// A menu sprite as straight RGBA over its cells' bounds (each cell's texture area stretched to its corners, its
    /// first corner colour applied, 0x80 = 1.0), for the modern layer's textures.
    pub fn sprite_rgba(&self, lib: &str, index: usize) -> Option<(u32, u32, Vec<u8>)> {
        let ui = &self.data.lobby;
        let s = ui.lib(lib)?.sprites.get(index)?;
        let xs = s.cells.iter().flat_map(|c| c.corners.iter().map(|p| p[0] as i32));
        let ys = s.cells.iter().flat_map(|c| c.corners.iter().map(|p| p[1] as i32));
        let (x0, x1, y0, y1) = (xs.clone().min()?, xs.max()?, ys.clone().min()?, ys.max()?);
        let (w, h) = ((x1 - x0).max(1) as usize, (y1 - y0).max(1) as usize);
        let mut out = vec![0u8; w * h * 4];
        for c in &s.cells {
            let Some(img) = ui.images.get(&c.texture) else { continue };
            let (l, t) = (c.corners[0][0] as i32 - x0, c.corners[0][1] as i32 - y0);
            let (cw, ch) = ((c.corners[3][0] - c.corners[0][0]).max(1) as i32, (c.corners[3][1] - c.corners[0][1]).max(1) as i32);
            let k = c.colors[0].map(|v| v as f32 / 128.0);
            for py in 0..ch {
                for px in 0..cw {
                    let (mut u, mut v) = (px * c.size[0] as i32 / cw, py * c.size[1] as i32 / ch);
                    if c.flip & 1 != 0 {
                        u = c.size[0] as i32 - 1 - u;
                    }
                    if c.flip & 2 != 0 {
                        v = c.size[1] as i32 - 1 - v;
                    }
                    let (u, v) = (c.uv[0] as i32 + u, c.uv[1] as i32 + v);
                    let (ox, oy) = (l + px, t + py);
                    if u < 0 || v < 0 || u >= img.width as i32 || v >= img.height as i32 || ox < 0 || oy < 0 || ox >= w as i32 || oy >= h as i32 {
                        continue;
                    }
                    let si = (v as usize * img.width as usize + u as usize) * 4;
                    let src: [f32; 4] = std::array::from_fn(|i| (img.rgba[si + i] as f32 * k[i]).min(255.0));
                    let di = (oy as usize * w + ox as usize) * 4;
                    // Over what is there.
                    let a = src[3] / 255.0;
                    for i in 0..3 {
                        out[di + i] = (src[i] * a + out[di + i] as f32 * (1.0 - a)) as u8;
                    }
                    out[di + 3] = (src[3] + out[di + 3] as f32 * (1.0 - a)).min(255.0) as u8;
                }
            }
        }
        Some((w as u32, h as u32, out))
    }

    pub fn status_rgba(&self, small: bool, id: u16) -> Option<(u32, u32, Vec<u8>)> {
        crate::dat::statusinfo::icon(self.data.status[small as usize].as_deref()?, id)
    }

    /// A status info entry (cancellable, never timed, description).
    pub fn status_entry(&self, small: bool, id: u16) -> Option<crate::dat::statusinfo::Entry> {
        crate::dat::statusinfo::entry(self.data.status[small as usize].as_deref()?, id)
    }

    /// An uploaded image stretched over a rect; colour 1.0 = 0x80.
    pub fn image_rect(&mut self, name: &str, x: f32, y: f32, w: f32, h: f32, rgba: [f32; 4]) {
        self.batch.push_image(name, x, y, w, h, rgba);
    }

    /// A menu window with every child's sprite, the child `sel` highlighted with the hand (FUN_101144b0).
    pub fn menu_sel(&mut self, name: &str, w: f32, h: f32, sel: u16) -> Option<(f32, f32)> {
        let (fx, fy, fw, fh) = screens::frame_rect(self.data.lobby.menu(name)?, w, h, 1.0, 1.0);
        if !self.win_gate(name, [fx, fy, fw, fh]) {
            return None;
        }
        screens::push_menu_sel(&mut self.batch, &self.data.lobby, &self.data.style, name, w, h, sel)
    }

    /// A HUD window at frame alpha a (skipped when 0); returns its position either way.
    pub fn window_alpha(&mut self, name: &str, w: f32, h: f32, a: f32) -> Option<(f32, f32)> {
        // A window the walk suspends draws nothing, its content neither (titlewin under persona's 0x30).
        if !self.want(name) {
            return None;
        }
        let m = self.data.lobby.menu(name)?;
        let pos = screens::window_pos(m, w, h, 1.0, 1.0);
        let (fx, fy, fw, fh) = screens::frame_rect(m, w, h, 1.0, 1.0);
        if !self.win_gate(name, [fx, fy, fw, fh]) || a <= 0.0 {
            return Some(pos);
        }
        let m = self.data.lobby.menu(name)?;
        Some(screens::push_window_alpha(&mut self.batch, &self.data.lobby, &self.data.style, m, w, h, 0.0, a))
    }

    /// The layout x of a menu's window.
    pub fn window_rect_x(&self, name: &str) -> f32 {
        self.data.lobby.menu(name).and_then(|m| m.elements.first()).map_or(0.0, |e| e.x as f32)
    }

    /// A menu's children (cursor stops) as (x, y, w, h) relative to the window.
    pub fn children(&self, name: &str) -> Vec<[f32; 4]> {
        self.data.lobby.menu(name).map_or(Vec::new(), |m| m.elements.iter().skip(1).map(|e| [e.x as f32, e.y as f32, e.w as f32, e.h as f32]).collect())
    }

    pub fn window_rect(&self, name: &str, w: f32, h: f32) -> Option<[f32; 4]> {
        let dy = self.stack_dy(name, w, h);
        screens::window_rect(&self.data.lobby, name, w, h).map(|[x, y, rw, rh]| [x, y + dy, rw, rh])
    }

    /// Splits a line into pieces no wider than `max`, at each '\n' and at spaces where possible.
    pub fn wrap(&self, line: &str, max: f32) -> Vec<String> {
        self.wrap_indent(line, max, false)
    }

    /// FUN_1012a4f0: each wrapped line starts with the last `1e xx` before it, so a colour carries over; with `indent`
    /// (wrapper mode 1) a continuation line starts with a full-width space and is narrower by its width.
    pub fn wrap_indent(&self, line: &str, max: f32, indent: bool) -> Vec<String> {
        let code = |c: &char| (crate::dat::font::CODE_BASE..crate::dat::font::CODE_BASE + 0x100).contains(&(*c as u32));
        let line_colour = crate::dat::font::colour_code(1);
        let mut cur = line_colour;
        let mut out = Vec::new();
        for (_, piece) in self.wrap_spans(line, max, indent) {
            let mut s = if out.is_empty() || !indent { String::new() } else { "\u{3000}".to_string() };
            if cur != line_colour {
                s.push(cur);
            }
            s.push_str(&piece);
            cur = piece.chars().filter(code).last().unwrap_or(cur);
            out.push(s);
        }
        out
    }

    /// The wrapped lines of `line` as `wrap_indent` cuts them, without its prefixes, with the byte offset each starts
    /// at.
    pub fn wrap_spans(&self, line: &str, max: f32, indent: bool) -> Vec<(usize, String)> {
        let rest = if indent { max - self.text_width("\u{3000}") } else { max };
        let mut out = Vec::new();
        let mut base = 0;
        for (k, part) in line.split('\n').enumerate() {
            let mut off = 0;
            for piece in self.wrap_part(part, if k == 0 { max } else { rest }, rest) {
                let at = part[off..].find(piece.as_str()).map_or(off, |i| off + i);
                off = at + piece.len();
                out.push((base + at, piece));
            }
            base += part.len() + 1;
        }
        out
    }

    /// Words to lines: the first at most `first` wide, the others `rest`.
    fn wrap_part(&self, line: &str, first: f32, rest: f32) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let max = |out: &Vec<String>| if out.is_empty() { first } else { rest };
        for word in line.split_inclusive(' ') {
            if !cur.is_empty() && self.text_width(&(cur.clone() + word.trim_end())) > max(&out) {
                out.push(std::mem::take(&mut cur).trim_end().to_string());
            }
            cur.push_str(word);
            // A single word wider than the line breaks anywhere.
            while self.text_width(cur.trim_end()) > max(&out) && cur.chars().count() > 1 {
                let mut head = String::new();
                for ch in cur.chars() {
                    if self.text_width(&format!("{head}{ch}")) > max(&out) {
                        break;
                    }
                    head.push(ch);
                }
                if head.is_empty() {
                    break;
                }
                cur = cur[head.len()..].to_string();
                out.push(head);
            }
        }
        if !cur.trim_end().is_empty() || out.is_empty() {
            out.push(cur.trim_end().to_string());
        }
        out
    }

    /// A window in the configured frame style.
    pub fn frame(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.frame_alpha(x, y, w, h, 1.0);
    }

    /// The frame at window alpha a, alpha blended while a < 1 (FUN_101144b0).
    pub fn frame_alpha(&mut self, x: f32, y: f32, w: f32, h: f32, a: f32) {
        let blend = if a < 1.0 { 0x44 } else { 0 };
        self.batch.push_frame(&self.data.lobby, &self.data.style, x, y, w, h, 1.0, 1.0, screens::alpha_tint(a), blend, false, false);
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, rgba: [f32; 4]) {
        self.batch.push_rect(x, y, w, h, rgba);
    }

    /// A window cursor sprite (`lib`, e.g. the buff window's anc_equi box) scaled about (x, y) (FUN_1011ab50; the
    /// buff list's 0.74).
    pub fn cursor_scaled(&mut self, lib: &str, x: f32, y: f32, s: f32) {
        let frame = self.cursor_frame();
        if let Some(anc) = self.data.lobby.lib(lib) {
            self.batch.push_sprite(&self.data.lobby, anc, frame % anc.sprites.len().max(1), x, y, s, s, 0x8080_8080, 0);
        }
    }

    /// The pointing hand; its sprite reaches 32 px left of (x, y).
    pub fn cursor(&mut self, x: f32, y: f32) {
        let frame = self.cursor_frame();
        if let Some(anc) = self.data.lobby.lib("anc     anc_s") {
            self.batch.push_sprite(&self.data.lobby, anc, frame % anc.sprites.len().max(1), x, y, 1.0, 1.0, 0x8080_8080, 0);
        }
    }

    /// Draws this frame's batch, over the lobby clear colour or (`over`) on top of the 3D scene.
    /// Draws the batch; `fade` is the 2D layer level, `scene` the lobby scene level the clear colour takes.
    #[allow(clippy::too_many_arguments)]
    /// `layout`: the menu size the batch was laid out at; `size`: the target's pixels.
    #[allow(clippy::too_many_arguments)]
    pub fn render(&mut self, gpu: &Gpu, format: wgpu::TextureFormat, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, layout: [u32; 2], size: [u32; 2], over: bool, fade: f32, scene: f32) {
        let r = self.renderer.get_or_insert_with(|| {
            let mut r = UiRenderer::new(gpu, format);
            r.load(gpu, &self.data.lobby);
            let a = &self.data.font.atlas;
            r.load_image(gpu, ATLAS, a.width, a.height, &a.rgba);
            for (name, t) in &self.data.font.textures {
                r.load_image(gpu, &crate::dat::ui::norm_name(name.as_bytes()), t.width, t.height, &t.rgba);
            }
            r
        });
        for (name, w, h, rgba) in self.pending.drain(..) {
            r.load_image(gpu, &name, w, h, &rgba);
        }
        for (name, w, h, rgba) in self.replaced.drain(..) {
            r.replace_image(gpu, &name, w, h, &rgba);
        }
        let c = LOBBY_CLEAR.map(|v| v as f64 / 255.0 * scene.clamp(0.0, 1.0) as f64);
        let clear = (!over).then_some(wgpu::Color { r: c[0], g: c[1], b: c[2], a: 1.0 });
        r.fade = fade;
        r.set_scene_depth(gpu, self.scene_depth.take().as_ref());
        r.render(gpu, encoder, view, layout, size, clear, &self.batch);
    }
}

/// Creator screen text and windows (chdummy / lobyhelp / chmkname handlers).
impl GameUi {
    /// A lobby message (groups 1, 6, 8, 0x1b) as Shift-JIS, or .
    pub fn lobby_msg_raw(&self, group: u8, id: usize, fallback: &str) -> Vec<u8> {
        self.data.lobby_msgs.get(&group).and_then(|m| m.raw(id)).unwrap_or_else(|| encode_sjis(fallback))
    }

    pub fn lobby_msg(&self, group: u8, id: usize, fallback: &str) -> String {
        crate::dat::font::decode_sjis(&self.lobby_msg_raw(group, id, fallback))
    }

    /// The help line of a creator step (lobyhelp +0x2c = step + 0x13 -> group 8 0x6b..0x6f, the name step group
    /// 0x1b id 4, the nation step 0x1a -> group 8 0x72; FUN_10236b40 and FUN_1022ef00).
    pub fn creator_help(&self, step: usize) -> String {
        match step {
            0..=4 => self.lobby_msg(8, 0x6b + step, ""),
            5 => self.lobby_msg(0x1b, 4, "Please enter a name."),
            _ => self.lobby_msg(8, 0x72, "Select a country to start in."),
        }
    }

    /// The help line in the lobby help bar (FUN_10236b40): centred when it fits in W - 16, else it waits 0x1e
    /// frames, then scrolls left 2 px a frame as "<4 ideographic spaces><text>" drawn twice end to end.
    /// Frames are taken at 30 per second (inferred: the client's default frame rate).
    ///  None = the wall time since the last call.
    pub fn help_line(&mut self, text: &str, w: f32, h: f32, color: [u8; 4], dt: Option<f32>) {
        let now = Instant::now();
        let dt = dt.unwrap_or_else(|| self.help.last.map_or(0.0, |t| (now - t).as_secs_f32().min(0.5)));
        self.help.last = Some(now);
        let Some(slot) = self.help_frame(w, h) else { return };
        if self.help.text != text {
            self.help = HelpScroll { text: text.to_string(), wait: 0x1e, offset: 0.0, acc: 0.0, last: self.help.last };
        }
        let width = self.text_width(text);
        if width <= w - 16.0 {
            self.text(text, ((w - width) / 2.0).trunc(), slot.y, 0, color);
            return;
        }
        let looped = format!("\u{3000}\u{3000}\u{3000}\u{3000}{text}");
        let cycle = self.text_width(&looped);
        self.help.acc += dt * 30.0;
        while self.help.acc >= 1.0 {
            self.help.acc -= 1.0;
            if self.help.wait > 0 {
                self.help.wait -= 1;
            } else {
                self.help.offset += 2.0;
                if self.help.offset >= cycle {
                    self.help.offset = 0.0;
                }
            }
        }
        let off = self.help.offset;
        self.text(&looped, -off, slot.y, 0, color);
        if cycle - off < w {
            self.text(&looped, cycle - off, slot.y, 0, color);
        }
    }

    /// Script-font text from lobbywin glyph sprites (FUN_102308d0): '0'-'9' -> 12.., 'A'-'Z' -> 22..,
    /// 'a'-'z' -> 48..; a few two-byte letters -> 0xa0..0xa7; anything else advances 6. Returns the end x.
    pub fn script_text(&mut self, s: &[u8], x: f32, y: f32) -> f32 {
        let Some(lib) = self.data.lobby.lib("menu    lobbywin") else { return x };
        let mut x = x;
        let mut i = 0;
        while i < s.len() {
            let b = s[i];
            let glyph = match b {
                b'0'..=b'9' => Some(b as usize - 0x24),
                b'A'..=b'Z' => Some(b as usize - 0x2b),
                b'a'..=b'z' => Some(b as usize - 0x31),
                0x81..=0x9f | 0xe0..=0xef | 0xfa..=0xfc => {
                    i += 1;
                    match s.get(i) {
                        Some(0xbe) => Some(0xa0),
                        Some(0xc3) => Some(0xa1),
                        Some(0xc7) => Some(0xa2),
                        Some(0xce) => Some(0xa3),
                        Some(0xd5) => Some(0xa4),
                        Some(0xc8) => Some(0xa7),
                        _ => {
                            i += 1;
                            continue;
                        }
                    }
                }
                _ => None,
            };
            match glyph {
                Some(g) => {
                    self.batch.push_sprite(&self.data.lobby, lib, g, x, y, 1.0, 1.0, 0x8080_8080, 0);
                    x += screens::sprite_width(lib, g);
                }
                None => x += 6.0,
            }
            i += 1;
        }
        x
    }

    /// Race name, stat block and Face / Hair / Size line (FUN_102306d0, drawn by lobyhelp while +0x14 == 1),
    /// from the layout origin (FUN_10159920 mode 0).
    pub fn creator_stats(&mut self, v: &CreatorView) {
        if let Some(s) = self.data.lobby.lib("menu    lobbywin").and_then(|l| l.sprites.get(11)) {
            // Sprite 11 cells 0..0x14: the STR..CHR labels.
            for c in s.cells.iter().take(0x14) {
                self.batch.push_cell(&self.data.lobby, c, 56.0, 216.0, 1.0, 1.0, 0x8080_8080, 0);
            }
        }
        let race = self.lobby_msg_raw(6, v.race, "");
        self.script_text(&race, 40.0, 184.0);
        let fill = |f: Vec<u8>, arg: &[u8]| -> Vec<u8> {
            let at = f.windows(2).position(|p| p[0] == b'%' && (p[1] == b'c' || p[1] == b's'));
            at.map_or(f.clone(), |a| [&f[..a], arg, &f[a + 2..]].concat())
        };
        let face = fill(self.lobby_msg_raw(1, 5, "Face %c "), &[b'1' + v.face as u8]);
        let hair = fill(self.lobby_msg_raw(1, 6, "Hair %s "), &self.lobby_msg_raw(1, 0xb + v.hair.min(1), "A"));
        let size = fill(self.lobby_msg_raw(1, 7, "Size %s "), &self.lobby_msg_raw(1, 8 + v.size.min(2), "S"));
        let mut x = self.script_text(&face, 30.0, 342.0);
        x = self.script_text(&hair, x + 10.0, 342.0);
        self.script_text(&size, x + 10.0, 342.0);
        let base = (v.job.min(5) + v.race.min(7) * 6) * 7;
        for i in 0..7 {
            if let Some(&n) = self.data.stats.get(base + i) {
                self.script_text(format!("{n:4}").as_bytes(), 84.0, 216.0 + 16.0 * i as f32);
            }
        }
    }

    /// A frameless window (registry flag 0x20, e.g. race1..8, nation1..3): its sprite, every cell.
    pub fn bare_window(&mut self, name: &str, w: f32, h: f32) {
        self.bare_window_off(name, w, h, (0.0, 0.0));
    }

    /// `bare_window` moved by `off`; its place once drawn (not while it opens).
    pub fn bare_window_off(&mut self, name: &str, w: f32, h: f32, off: (f32, f32)) -> Option<(f32, f32)> {
        let m = self.data.lobby.menu(name)?;
        let (x, y) = screens::window_pos(m, w, h, 1.0, 1.0);
        let (x, y) = (x + off.0, y + off.1);
        let (lib_name, k) = m.elements[0].refs.iter().find(|r| r.kind == 0).map(|r| (r.lib.clone(), r.index as usize))?;
        if !self.gate(name, Shape::Sprite(lib_name, k, x, y)) {
            return None;
        }
        let m = self.data.lobby.menu(name)?;
        if let Some(r) = m.elements[0].refs.iter().find(|r| r.kind == 0)
            && let Some(lib) = self.data.lobby.lib(&r.lib)
        {
            self.batch.push_sprite(&self.data.lobby, lib, r.index as usize, x, y, 1.0, 1.0, 0x8080_8080, 0);
        }
        Some((x, y))
    }

    /// menu netwait (FUN_102408a0), centred as the prompts: the status text in the frame rect, 28 px off the bottom.
    pub fn netwait_window(&mut self, text: &str, w: f32, h: f32) {
        let Some(win) = self.lobby_window("menu    netwait", w, h, true, None, false, &[]) else { return };
        let r = Self::clamp_rect(win.frame, w, h);
        self.text_block(text, [r[0], r[1], r[2], r[3] - 28.0]);
    }

    /// menu netbar (FUN_102409a0), centred, frameless: its track, then lobbywin 77 from the left edge at (2p, 0.99)
    /// while 0.01 < p < 1.1 (the centred scale of FUN_1011fc80 cancels the (0.5 - p) * 128 shift).
    pub fn netbar_window(&mut self, p: f32, w: f32, h: f32) {
        let off = (((w - 512.0) / 2.0).trunc(), ((h - 448.0) / 2.0).trunc());
        let Some((x, y)) = self.bare_window_off("menu    netbar", w, h, off) else { return };
        if p > 0.01
            && p < 1.1
            && let Some(lib) = self.data.lobby.lib("menu    lobbywin")
        {
            self.batch.push_sprite(&self.data.lobby, lib, 77, x, y, 2.0 * p, 0.99, 0x8080_8080, 0);
        }
    }

    /// Everything the creator draws besides the step menu and help line: the race title ("menu    raceN",
    /// FUN_1022cd80) and stats up to the name step, the name entry with the Namingway pointer, the nation emblem.
    pub fn creator_overlay(&mut self, w: f32, h: f32, v: &CreatorView) {
        if v.step <= 5 {
            self.bare_window(&format!("menu    race{}", v.race.min(7) + 1), w, h);
            self.creator_stats(v);
        }
        // The emblem stays up behind the confirm (FUN_10238f50); chmktown's text goes with its window.
        if v.step >= 6 {
            self.bare_window(&format!("menu    nation{}", v.nation.min(2) + 1), w, h);
        }
        if v.step == 6 {
            self.nation_text(v.nation.min(2), w, h);
        }
        if v.step == 5 && !v.name_hidden {
            self.name_entry(v.name, v.world.is_none(), w, h);
        }
    }

    /// The chmkname window's contents (FUN_1022d9b0): the name at window + (0x10, 4), first letter upper case and
    /// the rest lower (FUN_1022dc60); while `active`, the caret on for frames 16..29 of every 30 and sprite 140 at
    /// child 2 (FUN_1015ed80: only with the focus).
    pub fn name_entry(&mut self, name: &str, active: bool, w: f32, h: f32) {
        let Some(m) = self.data.lobby.menu("menu    chmkname") else { return };
        let (x, y) = screens::window_pos(m, w, h, 1.0, 1.0);
        let child = m.elements.iter().find(|e| e.id == 2).map(|c| (c.x as f32, c.y as f32));
        let name: String = name.chars().enumerate().map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() }).collect();
        self.text(&name, x + 16.0, y + 4.0, 0, WHITE);
        if !active {
            return;
        }
        if (self.started.elapsed().as_secs_f32() * 30.0) as u32 % 30 > 15 {
            let cx = x + 16.0 + self.text_width(&name);
            self.rect(cx, y + 5.0, 2.0, 14.0, [1.0, 1.0, 1.0, 0.9]);
        }
        if let Some((cx, cy)) = child
            && let Some(lib) = self.data.lobby.lib("menu    lobbywin")
        {
            self.batch.push_sprite(&self.data.lobby, lib, 140, x + cx, y + cy, 1.0, 1.0, 0x8080_8080, 0);
        }
    }

    /// A step window's frame and sprite without its children, as `menu` draws it (no open animation).
    pub fn window_bare_frame(&mut self, name: &str, w: f32, h: f32) {
        if let Some(m) = self.data.lobby.menu(name) {
            screens::push_window(&mut self.batch, &self.data.lobby, &self.data.style, m, w, h);
        }
    }

    /// The creator's world step, placed as retail from chmkname's point (FUN_10119110): chmkserv 4 px below its
    /// layout with the cursor row's world at (+0x10, 4) (FUN_10238960); then worldsel (8 px below) with the hand on
    /// the cursor row and the names at (+16, 4 + 16 i) (FUN_101f6a60), or chmkpass with the pass or 8 0x3e, a '|'
    /// over the caret's character (FUN_10238c60). Returns the list's rows that hold a world.
    pub fn world_step(&mut self, v: &WorldView, w: f32, h: f32) -> Vec<[f32; 4]> {
        let at = |gu: &Self, name: &str, dy: f32| gu.data.lobby.menu(name).map(|m| screens::window_pos(m, w, h, 1.0, 1.0)).map(|(x, y)| (x + 16.0, y + dy + 4.0));
        if self.lobby_window_off("menu    chmkserv", w, h, (0.0, 4.0), None, false, &[]).is_some()
            && let Some((x, y)) = at(self, "menu    chmkserv", 4.0)
            && let Some(world) = v.rows.get(v.cursor)
        {
            self.text(world, x, y, 0, WHITE);
        }
        let Some((pass, caret, on)) = v.pass else {
            let sel = (v.cursor - v.top + 1) as u16;
            let Some(win) = self.lobby_window_off("menu    worldsel", w, h, (0.0, 8.0), Some(sel), true, &[]) else { return Vec::new() };
            if let Some((x, y)) = at(self, "menu    worldsel", 8.0) {
                for (i, world) in v.rows.iter().skip(v.top).take(super::creator::WORLD_ROWS).enumerate() {
                    self.text(world, x, y + 16.0 * i as f32, 0, WHITE);
                }
            }
            return win.children.iter().take(v.rows.len() - v.top).map(|c| c.1).collect();
        };
        if self.lobby_window_off("menu    chmkpass", w, h, (0.0, 8.0), None, false, &[]).is_some()
            && let Some((x, y)) = at(self, "menu    chmkpass", 8.0)
        {
            if pass.is_empty() {
                let t = self.lobby_msg(8, 0x3e, "");
                self.text(&t, x, y, 0, WHITE);
            } else {
                self.text(pass, x, y, 0, WHITE);
                if on {
                    let cx = x + self.text_width(&pass[..caret]);
                    self.text("|", cx, y, 0, WHITE);
                }
            }
        }
        Vec::new()
    }

    /// The highlighted nation's description (chmktown draw FUN_1022e7b0): the body (group 8 0x1e / 0x29 / 0x34)
    /// as a block centred in the UI area, lines left-aligned 20 px apart; the title (0x1d / 0x28 / 0x33) centred
    /// with its top 24 px above the block.
    pub fn nation_text(&mut self, nation: usize, w: f32, h: f32) {
        let (title, body) = ([0x1d, 0x28, 0x33][nation], [0x1e, 0x29, 0x34][nation]);
        let body = self.lobby_msg(8, body, "");
        let lines: Vec<String> = body.split('\n').flat_map(|l| self.wrap(l, w)).collect();
        let n = lines.len().max(1) as f32;
        let bh = n * 16.0 + (n - 1.0) * 4.0;
        let widest = lines.iter().map(|l| self.text_width(l)).fold(0.0, f32::max);
        let (x, y) = (((w - widest) / 2.0).trunc(), ((h - bh) / 2.0).trunc());
        for (i, l) in lines.iter().enumerate() {
            self.text(l, x, y + 20.0 * i as f32, 0, WHITE);
        }
        let t = self.lobby_msg(8, title, "");
        let c = (h / 2.0).trunc() - (bh / 2.0).trunc();
        self.text(&t, (w / 2.0).trunc(), c - 24.0, 1, WHITE);
    }
}
