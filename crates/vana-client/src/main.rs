// Windows release builds open no console window; main() attaches to the parent one when started from a terminal.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod audio;
mod cabx;
mod client;
mod combatshot;
mod config;
mod eventdump;
mod gamecfg;
mod flashshot;
mod fx;
mod event_vm;
mod itemshot;
mod creatorshot;
mod dialogshot;
mod dat;
mod install;
mod patch;

/// unrar_sys's build script picks its sources by the host (`cfg!(windows)`), so a MinGW cross build from Linux
/// leaves out UnRAR's isnt.cpp; these are its two functions, under their C++ names.
#[cfg(all(windows, target_env = "gnu"))]
mod unrar_isnt {
    /// `DWORD WinNT()`: Windows 10 (WNT_W10), the oldest this client supports.
    #[unsafe(export_name = "_Z5WinNTv")]
    pub extern "C" fn win_nt() -> u32 {
        0x0a00
    }

    /// `bool IsWindows11OrGreater()`: UnRAR's own stub answers false.
    #[unsafe(export_name = "_Z20IsWindows11OrGreaterv")]
    pub extern "C" fn is_windows_11_or_greater() -> bool {
        false
    }
}
mod motion;
mod modpack;
mod msi_db;
mod net;
mod render;
mod secret;
mod sounddump;
mod uishot;
mod inventory;
mod invshot;
mod invitems;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[derive(Parser)]
#[command(version, about = "Native FINAL FANTASY XI installer and client")]
struct Cli {
    #[command(subcommand)]
    /// Without a subcommand the game client opens.
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Install the game data from FFXIFullSetup_US.part1.exe (or an extracted copy).
    Install {
        source: PathBuf,
        #[arg(long)]
        game_dir: Option<PathBuf>,
        /// Where the RAR volumes are unpacked before installing (removed afterwards).
        #[arg(long)]
        staging: Option<PathBuf>,
        #[arg(long)]
        no_verify: bool,
        #[arg(long)]
        keep_staging: bool,
    },
    /// Bring the game files up to the current retail version from the PlayOnline patch server (no login).
    /// Data and code files (the client reads its tables out of the current FFXiMain.dll).
    Update {
        /// Ask the server and check the files, but download nothing.
        #[arg(long)]
        dry_run: bool,
        /// Leave executables and libraries as they are.
        #[arg(long)]
        data_only: bool,
        /// Patch-server connections at once (1..6).
        #[arg(long, default_value_t = 4)]
        connections: usize,
        /// Only fetch this manifest path (e.g. FFXiMain.dll) into --to, leaving the install alone.
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        to: Option<PathBuf>,
    },
    /// Open the client window and drive it: log in, pick a character, walk, screenshot, log out.
    Play {
        /// JSON file with "username" and "password".
        #[arg(long)]
        credentials: Option<PathBuf>,
        #[arg(long)]
        character: Option<String>,
        /// Hold W for this many seconds after the zone loads.
        #[arg(long, default_value_t = 0.0)]
        walk: f32,
        /// Waypoints to run through: "x,z;x,z;..."
        #[arg(long, allow_hyphen_values = true)]
        goto: Option<String>,
        /// Save the window contents here, then log out and close.
        #[arg(long)]
        screenshot: Option<PathBuf>,
        /// Seconds after the zone loads to take the screenshot.
        #[arg(long, default_value_t = 5.0)]
        after: f32,
        /// After the screenshot, log out and close instead of leaving the window up.
        #[arg(long)]
        quit: bool,
        /// Scripted input, e.g. "until:login;user;key:Tab;pass;key:Enter;until:title;shot:a.png", or @FILE holding it
        /// (see client/script.rs).
        #[arg(long, allow_hyphen_values = true)]
        input: Option<String>,
        /// While engaged, walk up to the target.
        #[arg(long)]
        chase: bool,
    },
    /// Dump the structure of one MMB in a zone by name.
    MmbDump { zone: u32, name: String },
    /// Waypoints between two ground positions with the client's walk collision (A* on a 1-unit grid),
    /// printed as a --goto string.
    ZonePath {
        zone: u32,
        /// Start x,y,z (feet).
        #[arg(long, allow_hyphen_values = true)]
        from: String,
        /// Goal x,z.
        #[arg(long, allow_hyphen_values = true)]
        to: String,
    },
    /// List the scenery models whose triangles cover a ground position (x, z).
    ZoneProbe {
        zone: u32,
        #[arg(allow_hyphen_values = true)]
        x: f32,
        #[arg(allow_hyphen_values = true)]
        z: f32,
    },
    /// Render the title flyover (file 23): every camera op of one scheduler (mov1..mov8, ex1a..) at a few times.
    FlyoverShot {
        scheduler: String,
        #[arg(long)]
        out_dir: PathBuf,
        /// Samples per camera path (t = 0..1).
        #[arg(long, default_value_t = 3)]
        steps: u32,
        /// Skip retail lighting (placeholder shading).
        #[arg(long)]
        flat: bool,
        #[arg(long, default_value_t = 1280)]
        width: u32,
        #[arg(long, default_value_t = 720)]
        height: u32,
    },
    /// Render the character select screen (zone 133 ring, camera, list, windows) through client/select.rs.
    SelectShot {
        #[arg(long)]
        out: PathBuf,
        /// The cursor moves to this list slot after --hold frames.
        #[arg(long, default_value_t = 0)]
        slot: usize,
        /// Characters in the list (races cycle Hume M .. Galka; three empty slots follow).
        #[arg(long, default_value_t = 3)]
        chars: usize,
        /// Lobby frames (30 a second) before the cursor moves, and after it.
        #[arg(long, default_value_t = 0)]
        hold: u32,
        #[arg(long, default_value_t = 0)]
        frames: u32,
        /// Run until the camera has settled on the slot (60 frames after the move).
        #[arg(long)]
        close: bool,
        /// Window state: list, options, slots, login, delete, reset, settings, ok, rename.
        #[arg(long, default_value = "list")]
        ui: String,
        /// The delete screen instead.
        #[arg(long)]
        delete: bool,
        /// List slots drawn as must-rename (yellow) / cannot-use (grey).
        #[arg(long)]
        rename: Option<usize>,
        #[arg(long)]
        unusable: Option<usize>,
        /// The characters' zone (the info panel's Area).
        #[arg(long, default_value_t = 230)]
        zone: u16,
        /// Select screen time is 15:30 (FUN_1023b360(15, 30)).
        #[arg(long, default_value_t = 15.5)]
        hour: f32,
        #[arg(long, default_value_t = 1280)]
        width: u32,
        #[arg(long, default_value_t = 720)]
        height: u32,
    },
    /// List the chunks of a game DAT by file id (the first 12 unless --all).
    DatInfo {
        id: u32,
        #[arg(long)]
        all: bool,
    },
    /// Print English client messages of a group (thunk_FUN_1014c660(group, id)); all when no ids given.
    MsgDump { group: u8, ids: Vec<usize> },
    /// Decode a menu DAT (default 79, the US lobby): images, sprite sheets and a listing.
    UiDump {
        #[arg(long, default_value_t = dat::ui::LOBBY_US)]
        id: u32,
        #[arg(long)]
        out: PathBuf,
    },
    /// Render the title screen (lobby main menu) offscreen.
    TitleShot {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 1280)]
        width: u32,
        #[arg(long, default_value_t = 720)]
        height: u32,
    },
    /// Render the inventory list and gil windows offscreen (placeholder items).
    InventoryShot {
        #[arg(long)]
        out: PathBuf,
        /// First visible row.
        #[arg(long, default_value_t = 0)]
        scroll: usize,
        /// Cursor row on screen.
        #[arg(long, default_value_t = 0)]
        cursor: usize,
        #[arg(long, default_value_t = 0)]
        frame: usize,
        #[arg(long, default_value_t = 512)]
        width: u32,
        #[arg(long, default_value_t = 448)]
        height: u32,
    },
    /// Render the equipment, status and gil windows offscreen (placeholder items).
    EquipmentShot {
        #[arg(long)]
        out: PathBuf,
        /// Cursor child (1..16).
        #[arg(long, default_value_t = 1)]
        cursor: usize,
        #[arg(long, default_value_t = 0)]
        frame: usize,
        #[arg(long, default_value_t = 512)]
        width: u32,
        #[arg(long, default_value_t = 448)]
        height: u32,
    },
    /// Render a mock in-game HUD offscreen.
    /// Render the top bars (titlewin, helpwind, netstat) with the main menu, party and target windows offscreen.
    TopbarShot {
        #[arg(long)]
        out: PathBuf,
        /// Fade state: shown, hidden, in:N or out:N (N ticks left of 20).
        #[arg(long, default_value = "shown")]
        fade: String,
        /// Main menu child (1..12) under the cursor; none = menu closed.
        #[arg(long)]
        menu: Option<u16>,
        /// Main menu children whose label the menu draws itself, CHILD:WINDOWPS (the Mog House row: 12:44, visiting
        /// another's house 12:443).
        #[arg(long)]
        menu_swap: Option<String>,
        /// Target window: "mob" (gauge) or "npc" (no gauge).
        #[arg(long)]
        target: Option<String>,
        #[arg(long, default_value_t = 100)]
        hpp: u8,
        /// The map window: ZONE,X,Z[,owned] (the player's world x / z); ZONE,X,Z,overlay the map overlay;
        /// ZONE,X,Z,minimap[,DOCK,SIZE,SPAN] the minimap with sample dots under the HUD.
        #[arg(long, allow_hyphen_values = true)]
        map: Option<String>,
        /// Only the "Downloading data ..." indicator at its draw N (from 1).
        #[arg(long)]
        downloading: Option<usize>,
        /// Extra windows by menu name and highlighted child, e.g. "configwi:1,conf2win:3".
        #[arg(long)]
        window: Option<String>,
        /// The Config windows as the client draws them, bottom first: "configwi:7,conf7:3"; "lobycwin" over the
        /// title; "settings:TAB:ROW" for our settings screen; a list page takes ":CURSOR:SCROLL".
        #[arg(long)]
        config: Option<String>,
        /// Config values for --config: "0x07=1,0x13=2".
        #[arg(long)]
        cfgset: Option<String>,
        /// Installed-content mask for lobycwin (default: this install's ROMs), e.g. 0x7ff.
        #[arg(long, value_parser = parse_mask)]
        installed: Option<u32>,
        #[arg(long, default_value_t = 1280)]
        width: u32,
        #[arg(long, default_value_t = 720)]
        height: u32,
    },
    UiShot {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 1280)]
        width: u32,
        #[arg(long, default_value_t = 720)]
        height: u32,
    },
    /// A GPU-skinned Hume in a zone at an hour with the target flash at k 0 / 0.5 / 1 (flash_k*.png in --out-dir).
    FlashShot {
        #[arg(long, default_value_t = 101)]
        zone: u32,
        #[arg(long, default_value_t = 0.0)]
        hour: f32,
        /// The actor's feet (world x, y, z).
        #[arg(long, allow_hyphen_values = true, default_value = "82.339,-62.0,269.878")]
        pos: String,
        #[arg(long)]
        out_dir: PathBuf,
    },
    /// Replay LSB-built 0x028 / 0x029 / 0x02D packets into the chat log, with the action menu, to a PNG.
    CombatShot {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "Wild Rabbit")]
        mob: String,
        /// Action menu layout (FFXiMain 0x10363648): 0 no target, 2 other target, 4 monster, 6 engaged.
        #[arg(long, default_value_t = 4)]
        layout: usize,
        #[arg(long, default_value_t = 1280)]
        width: u32,
        #[arg(long, default_value_t = 720)]
        height: u32,
    },
    /// Render a zone dialogue line in the chat log window (and optionally a choice menu) to a PNG.
    DialogueShot {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 230)]
        zone: u16,
        /// Message index in the zone's dialogue file.
        #[arg(long, default_value_t = 7959)]
        id: u32,
        #[arg(long, default_value = "Taumila")]
        speaker: String,
        /// Also open the query menu with this message (question, 0x0b, options).
        #[arg(long)]
        choice: Option<u32>,
        /// Prompt arrow / cursor animation frame.
        #[arg(long, default_value_t = 0)]
        frame: usize,
        #[arg(long, default_value_t = 512)]
        width: u32,
        #[arg(long, default_value_t = 448)]
        height: u32,
    },
    /// Render a character (race 1..8 and lobby look ids) offscreen to a PNG.
    CharShot {
        #[arg(long, default_value_t = 1)]
        race: u8,
        #[arg(long, default_value_t = 0)]
        face: u16,
        #[arg(long, default_value_t = 0)]
        head: u16,
        #[arg(long, default_value_t = 8)]
        body: u16,
        #[arg(long, default_value_t = 8)]
        hands: u16,
        #[arg(long, default_value_t = 8)]
        legs: u16,
        #[arg(long, default_value_t = 8)]
        feet: u16,
        #[arg(long, default_value_t = 0)]
        main: u16,
        #[arg(long, default_value_t = 0)]
        sub: u16,
        /// The range slot's model (drawn: no wep2 hide here).
        #[arg(long, default_value_t = 0)]
        range: u16,
        /// Motion: 3 letters for all body regions (idl = idl0+idl1+idl2), a chunk name, or an index.
        #[arg(long)]
        anim: Option<String>,
        #[arg(long, default_value_t = 0.0)]
        frame: f32,
        /// Camera heading around the character in degrees (90 = facing the camera).
        #[arg(long, default_value_t = 60.0, allow_hyphen_values = true)]
        yaw: f32,
        /// Extra motion files (event banks, battle files) searched before the model's, last first.
        #[arg(long, num_args = 1..)]
        motions: Vec<u32>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Render one frame of the character creator preview (dancer engine) to a PNG.
    CreatorShot {
        /// Creator race 0..7: HumeM HumeF ElvaanM ElvaanF TaruM TaruF Mithra Galka.
        #[arg(long, default_value_t = 0)]
        race: usize,
        #[arg(long, default_value_t = 0)]
        face: usize,
        /// 0 = A, 1 = B.
        #[arg(long, default_value_t = 0)]
        hair: usize,
        /// Performance frame, 30 per second.
        #[arg(long, default_value_t = 0.0)]
        frame: f32,
        /// 0/1 = recorded camera 0/1, 2 = idle loop, front camera.
        #[arg(long, default_value_t = 0)]
        view: u8,
        #[arg(long, default_value_t = 640)]
        width: u32,
        #[arg(long, default_value_t = 480)]
        height: u32,
        /// Transparent background, for compositing over the zone-133 backdrop.
        #[arg(long)]
        transparent: bool,
        /// Draw over this PNG (same size as the output).
        #[arg(long)]
        backdrop: Option<PathBuf>,
        /// Old flat preview (one light, no shader passes).
        #[arg(long)]
        flat: bool,
        /// The whole creator screen as the client draws it: zone, dancer and the step's 2D layer.
        #[arg(long)]
        screen: bool,
        /// With --screen: creator step 0 race .. 5 name, 6 nation, 7 confirm.
        #[arg(long, default_value_t = 0)]
        step: usize,
        #[arg(long, default_value_t = 0)]
        size: usize,
        #[arg(long, default_value_t = 0)]
        job: usize,
        #[arg(long, default_value_t = 0)]
        nation: usize,
        #[arg(long, default_value = "")]
        name: String,
        /// With --screen: seconds of help-line scrolling before the shot.
        #[arg(long, default_value_t = 0.0)]
        help_time: f32,
        /// With --screen: also time this many 60 Hz frames.
        #[arg(long, default_value_t = 0)]
        bench: u32,
        #[arg(long)]
        out: PathBuf,
    },
    /// Disassemble a zone's event scripts (event DAT), with dialogue text inlined.
    EventDump {
        zone: u16,
        /// Only this actor (server id or its low 12-bit index).
        #[arg(long, value_parser = parse_num)]
        actor: Option<u32>,
        /// Only actors that have this event id (csid).
        #[arg(long, value_parser = parse_num)]
        event: Option<u32>,
    },
    /// List the event VM opcode table.
    EventOps,
    /// Regenerate data/msgshift.txt: LSB IDs.lua text ids matched against this install's dialogue DATs.
    MsgshiftGen {
        /// LandSandBoat checkout (reads scripts/zones/*/IDs.lua and scripts/enum/zone.codegen.lua).
        #[arg(long)]
        lsb: PathBuf,
        #[arg(long, default_value = "crates/vana-client/data/msgshift.txt")]
        out: PathBuf,
    },
    /// Map LSB text ids of a zone to installed dialogue ids with the bundled table.
    MsgshiftMap { zone: u16, ids: Vec<u32> },
    /// Print item records (name, flags, slots, jobs, level, description); --key: key items.
    ItemDump {
        #[arg(long, default_value = "en")]
        lang: String,
        #[arg(long)]
        key: bool,
        #[arg(value_parser = parse_num)]
        ids: Vec<u32>,
    },
    /// Render item icons at 4x into one PNG.
    ItemIconShot {
        #[arg(value_parser = parse_num)]
        ids: Vec<u32>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "en")]
        lang: String,
    },
    /// Run every event of the given zones (all when none) headless and count finished / faults.
    EventSweep { zones: Vec<u16> },
    /// Run one event headless (event VM) and print its effects.
    EventRun {
        zone: u16,
        #[arg(value_parser = parse_num)]
        event: u32,
        /// Target UniqueNo (0x034 target).
        #[arg(long, value_parser = parse_num, default_value_t = 0)]
        target: u32,
        /// Choice answers in order (option index; -1 = cancel).
        #[arg(long, allow_hyphen_values = true)]
        choice: Vec<i32>,
        /// 0x034 params (num[8]).
        #[arg(long, value_parser = parse_num)]
        param: Vec<u32>,
        /// The player's race (1 Hume M .. 8 Galka) for value 0x7f87.
        #[arg(long, default_value_t = 0)]
        race: u8,
    },
    /// Decode a .bgw / .spw to WAV: --id N (effect id, or music number with --music) or --file.
    SoundDump {
        #[arg(long)]
        id: Option<u32>,
        #[arg(long)]
        music: bool,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
        /// Decode this many seconds (looping) instead of one pass to the end.
        #[arg(long)]
        seconds: Option<f32>,
    },
    /// Play a music track (or with --se an effect) through the audio device for N seconds.
    AudioTest {
        #[arg(default_value_t = 0)]
        id: u32,
        #[arg(long, default_value_t = 10.0)]
        seconds: f32,
        #[arg(long)]
        se: bool,
        /// Fade the track out over this many 60 Hz ticks at the end.
        #[arg(long, default_value_t = 60)]
        fade: i32,
        /// cpal device id (see --list); default: the config's, else the system default.
        #[arg(long)]
        device: Option<String>,
        /// List output devices and exit.
        #[arg(long)]
        list: bool,
    },
    /// Render the camera of an event scheduler (event opcode 0x45: camera file n, tag) in a zone.
    CsShot {
        zone: u32,
        /// Opcode 0x45 file number (file id 0x77f0 + n, remapped from 300).
        file: u32,
        scheduler: String,
        #[arg(long)]
        out_dir: PathBuf,
        #[arg(long, default_value_t = 3)]
        steps: u32,
        #[arg(long, default_value_t = 1280)]
        width: u32,
        #[arg(long, default_value_t = 720)]
        height: u32,
    },
    /// Render an NPC or monster (0x00E model id, or a 20-byte LSB look as hex) offscreen to a PNG.
    NpcShot {
        #[arg(long)]
        model: Option<u16>,
        /// LSB look column, e.g. 0x0100030212201230...
        #[arg(long)]
        look: Option<String>,
        /// Motion: 3 letters for all body regions, a chunk name, several of them comma-separated (posed in order, the last
        /// winning), or an index. Default idl.
        #[arg(long, default_value = "idl")]
        anim: String,
        #[arg(long, default_value_t = 0.0)]
        frame: f32,
        /// Camera heading around the model in degrees (90 = facing the camera).
        #[arg(long, default_value_t = 60.0, allow_hyphen_values = true)]
        yaw: f32,
        /// Extra motion files (event banks, effect files) searched before the model's, last first.
        #[arg(long, num_args = 1..)]
        motions: Vec<u32>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Render text in the game font offscreen to a PNG. Each --text is one line; "\xHH" inserts a raw
    /// Shift-JIS byte (e.g. "\xEF\x1F" is the fire icon).
    TextShot {
        #[arg(long, required = true)]
        text: Vec<String>,
        /// compressed (the menu DAT's DXT3 atlas), uncompressed or high (the PNG atlases).
        #[arg(long, default_value = "compressed")]
        font: String,
        /// ja, en, fr or de: picks the menu DAT.
        #[arg(long, default_value = "en")]
        lang: String,
        /// Vertex colour as BBGGRRAA hex, 0x80 = 1.0 (the client default is 80808080).
        #[arg(long, default_value = "80808080")]
        color: String,
        /// Background as RRGGBB hex.
        #[arg(long, default_value = "1a2040")]
        bg: String,
        /// Blend glyphs with alpha instead of the client's additive ONE/ONE.
        #[arg(long)]
        alpha: bool,
        /// Integer zoom of the saved image.
        #[arg(long, default_value_t = 1)]
        zoom: u32,
        #[arg(long)]
        out: PathBuf,
    },
    /// Parse a zone's scenery and report how placements, meshes and textures line up.
    ZoneInfo { zone: u32 },
    /// An action's effects offscreen: a Hume M (fists) at --caster acts on a model-1300 orc at --victim in a
    /// zone; frames every --every ticks after it starts. Category 1 = melee, else the effect file of --anim.
    FxShot {
        zone: u32,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
        eye: Vec<f32>,
        #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
        caster: Vec<f32>,
        #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
        victim: Vec<f32>,
        #[arg(long, default_value_t = 1)]
        category: u8,
        #[arg(long, default_value_t = 0)]
        anim: u16,
        /// Frames to write, and ticks between them.
        #[arg(long, default_value_t = 6)]
        frames: u32,
        #[arg(long, default_value_t = 8)]
        every: u32,
        #[arg(long, default_value_t = 12.0)]
        hour: f32,
        /// Instead of the action, a walking footstep at the caster that bursts this dust generator (e.g. w100).
        #[arg(long)]
        dust: Option<String>,
        /// Instead of the action, effect file N's "main" on the caster (e.g. 3310, level up).
        #[arg(long)]
        file: Option<u32>,
        /// The caster moves by this much a tick once the effect starts.
        #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
        walk: Vec<f32>,
    },
    /// Effects audit: every weapon skill, mob TP move, spell, ability and item effect file run headless ("main" of
    /// base + animation, as 0x028 starts it) against stand-in actors; mob moves cast by a model that uses them (a TSV
    /// of animation, look hex). Writes one row per file and a ranked summary of skipped ops and unresolved parts.
    FxSweep {
        #[arg(long)]
        out: PathBuf,
        /// ws, mob, spell, ja, item (comma separated); all by default.
        #[arg(long, default_value = "ws,mob,spell,ja,item")]
        kinds: String,
        /// Ticks to run each file (it ends sooner when no task is left).
        #[arg(long, default_value_t = 900)]
        ticks: u32,
        #[arg(long)]
        mob_models: Option<PathBuf>,
        /// Only this category,animation (e.g. 11,79), printing its schedulers' ops.
        #[arg(long, value_delimiter = ',')]
        only: Vec<u32>,
    },
    /// Render a zone offscreen to a PNG. Coordinates are game units (Y points down).
    ZoneShot {
        zone: u32,
        #[arg(long)]
        out: PathBuf,
        /// Vana'diel hour (e.g. 11.5) to apply the zone's retail lighting; omit for the placeholder shading.
        #[arg(long)]
        hour: Option<f32>,
        #[arg(long, default_value = "suny")]
        weather: String,
        /// Camera position as x,y,z; default is high above the zone centre.
        #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
        eye: Option<Vec<f32>>,
        #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
        target: Option<Vec<f32>>,
        /// Run the zone's effect generators for this many 60 Hz ticks first and draw them.
        #[arg(long)]
        effects: Option<u32>,
        /// Moon phase 0..11 for the effects (0 = new moon).
        #[arg(long, default_value_t = 0)]
        moon: u32,
        /// Also start this generator of the zone (4cc) with the effects, for checking one that is not auto-started.
        #[arg(long)]
        start: Option<String>,
        /// Load this sub-area (building interior) over the zone, hiding its stand-ins.
        #[arg(long)]
        sub: Option<u32>,
        /// Also start an event scheduler in the effects as FILE:TAG (DAT id, 4cc), as event opcode 0x45 does.
        #[arg(long)]
        sched: Option<String>,
        /// A ship zone's voyage at this progress 0..1 (`r` after it: reversed), on its `c000` path.
        #[arg(long)]
        voyage: Option<String>,
    },
    /// Dump every table of an MSI as TSV, for inspection.
    DumpMsi { msi: PathBuf, out: PathBuf },
    /// Create an account; writes a generated password to the credentials file.
    AccountCreate {
        #[arg(long)]
        credentials: PathBuf,
        #[arg(long)]
        user: String,
    },
    /// Log in headless: list characters, optionally create one or select one for the map server.
    Login {
        /// JSON file with "username" and "password".
        #[arg(long)]
        credentials: PathBuf,
        #[arg(long)]
        create_char: Option<String>,
        #[arg(long, default_value_t = 1)]
        race: u8,
        #[arg(long, default_value_t = 0)]
        face: u8,
        #[arg(long, default_value_t = 1)]
        size: u8,
        #[arg(long, default_value_t = 1)]
        job: u8,
        #[arg(long, default_value_t = 0)]
        nation: u8,
        #[arg(long)]
        select: Option<String>,
        /// After selecting, zone in and stay this many seconds, then log out.
        #[arg(long)]
        zone_in: Option<u64>,
    },
}

/// Headless zone-in: logs what the server sends and keeps the session alive with 0x015.
fn zone_in(cfg: &config::Config, m: &net::lobby::MapLogin, account: &str, stay: u64) -> Result<()> {
    use net::map::session::{MapSession, build};
    use std::collections::BTreeMap;
    use std::time::Instant;

    let codec = net::map::compress::Codec::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?)?;
    let addr = std::net::SocketAddr::from((m.map.0, m.map.1));
    let mut map = MapSession::connect(addr, &m.key, codec)?;
    // The zone server learns of us a moment after the lobby replies, so retry like the client does.
    let mut attempt = 1;
    map.send_login(m.char_id, &m.name, account, attempt)?;
    let first = loop {
        if let Some(packets) = map.recv(Duration::from_millis(500))? {
            break packets;
        }
        attempt += 1;
        if attempt > 20 {
            anyhow::bail!("the zone server did not answer the login");
        }
        map.send_login(m.char_id, &m.name, account, attempt)?;
    };
    println!("zone server answered after {attempt} attempt(s)");
    let mut backlog = Some(first);

    let start = Instant::now();
    let mut counts: BTreeMap<u16, usize> = BTreeMap::new();
    let mut pos: Option<[u8; 12]> = None;
    let mut queue: Vec<Vec<u8>> = Vec::new();
    let mut last_send = Instant::now();
    let mut logout_sent = false;
    while start.elapsed() < Duration::from_secs(stay + 45) {
        let received = match backlog.take() {
            Some(p) => Some(p),
            None => map.recv(Duration::from_millis(100))?,
        };
        if let Some(packets) = received {
            for p in packets {
                *counts.entry(p.id).or_default() += 1;
                match p.id {
                    0x00A => {
                        println!(
                            "0x00A zone-in: {} zone {} pos ({:.2}, {:.2}, {:.2}) dir {} index {}",
                            p.str(132, 16), p.u32(48), p.f32(12), p.f32(16), p.f32(20), p.u8(11) as i8, p.u16(8)
                        );
                        pos = Some(p.data[12..24].try_into().unwrap());
                        queue.push(build(0x00C, &[0u8; 8]));
                        queue.push(build(0x011, &[2, 0, 0, 0]));
                        let (event_num, event_para) = (p.u16(98), p.u16(100));
                        if event_para != 0 {
                            // A login cutscene (e.g. a new character's intro); finish it so the session is usable.
                            println!("login event {event_para} (EventNum {event_num}, mode {}) - skipping it", p.u16(102));
                            let mut end = Vec::with_capacity(16);
                            end.extend_from_slice(&p.u32(4).to_le_bytes());
                            end.extend_from_slice(&0u32.to_le_bytes());
                            end.extend_from_slice(&p.u16(8).to_le_bytes());
                            end.extend_from_slice(&0u16.to_le_bytes());
                            end.extend_from_slice(&event_num.to_le_bytes());
                            end.extend_from_slice(&event_para.to_le_bytes());
                            queue.push(build(0x05B, &end));
                        }
                    }
                    0x00B => {
                        println!("0x00B logout/zone (state {}) after {:.1}s", p.u32(4), start.elapsed().as_secs_f32());
                        println!("received: {}", counts.iter().map(|(id, n)| format!("{id:03X}x{n}")).collect::<Vec<_>>().join(" "));
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
        if !logout_sent && start.elapsed() > Duration::from_secs(stay) {
            println!("requesting logout");
            queue.push(build(0x0E7, &[1, 0, 1, 0]));
            logout_sent = true;
        }
        if last_send.elapsed() > Duration::from_millis(400) {
            if let Some(p) = pos {
                let mut body = [0u8; 28];
                body[..12].copy_from_slice(&p);
                queue.push(build(0x015, &body));
            }
            map.send(&queue)?;
            queue.clear();
            last_send = Instant::now();
        }
    }
    println!("received: {}", counts.iter().map(|(id, n)| format!("{id:03X}x{n}")).collect::<Vec<_>>().join(" "));
    anyhow::bail!("no logout confirmation from the zone server")
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

fn random_password() -> Result<String> {
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("random bytes: {e}"))?;
    Ok(bytes.iter().map(|b| CHARS[*b as usize % CHARS.len()] as char).collect())
}

/// Owner-only on Unix (0600); Windows keeps user files private by default.
fn write_private(path: &PathBuf, data: &[u8]) -> Result<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let mut f = o.open(path)?;
    std::io::Write::write_all(&mut f, data)?;
    Ok(())
}

fn parse_mask(s: &str) -> Result<u32, String> {
    s.strip_prefix("0x").map_or_else(|| s.parse(), |h| u32::from_str_radix(h, 16)).map_err(|e| e.to_string())
}

fn login(cfg: &config::Config, creds: &Credentials) -> Result<net::lobby::Lobby> {
    use net::auth::{self, Command, Reply, Request};
    let req = Request { username: &creds.username, password: &creds.password, ..Default::default() };
    let Reply::LoggedIn { account_id, session_hash, .. } = auth::send(&cfg.server, cfg.auth_port, Command::Login, &req)? else {
        anyhow::bail!("unexpected login reply");
    };
    println!("Logged in: account {account_id}");
    let lobby =
        net::lobby::Lobby::connect(&cfg.server, cfg.view_port, cfg.data_port, account_id, session_hash, &cfg.client_version)?;
    println!("Lobby: expansions 0x{:08X}, features 0x{:08X}", lobby.expansions, lobby.features);
    Ok(lobby)
}

/// Log lines to stderr and a file.
struct Tee(std::fs::File);

impl std::io::Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        self.0.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stderr().flush();
        self.0.flush()
    }
}

fn main() -> Result<()> {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS);
    }
    let cli = Cli::parse();
    // The client also logs to a file (the last run's kept as .prev), its effect lines included for now; Windows logs
    // every command (a GUI exe started without a console loses stderr). The offscreen tools leave the file alone.
    let client = matches!(cli.cmd, None | Some(Cmd::Play { .. }));
    let mut log = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(if client { "info,vana_client::fx=debug" } else { "info" }));
    log.format_timestamp_millis();
    let path = config::log_path();
    if (client || cfg!(windows))
        && let Ok(file) = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| {
            let _ = std::fs::rename(&path, path.with_extension("prev.log"));
            std::fs::File::create(&path)
        })
    {
        log.target(env_logger::Target::Pipe(Box::new(Tee(file))));
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log::error!("{info}\n{}", std::backtrace::Backtrace::force_capture());
            default(info);
        }));
    }
    log.init();
    let Some(cmd) = cli.cmd else {
        return client::run(config::Config::load(), client::Drive::default());
    };
    match cmd {
        Cmd::Play { credentials, character, walk, goto, screenshot, after, quit, input, chase } => {
            let goto: Vec<(f32, f32)> = goto
                .unwrap_or_default()
                .split(';')
                .filter_map(|p| p.split_once(',').and_then(|(x, z)| Some((x.trim().parse().ok()?, z.trim().parse().ok()?))))
                .collect();
            let login = match credentials {
                Some(path) => {
                    let c: Credentials = serde_json::from_slice(&std::fs::read(path)?)?;
                    Some((c.username, c.password))
                }
                None => None,
            };
            // "@path" reads the script from a file.
            let input = input.map(|s| match s.strip_prefix('@') { Some(p) => std::fs::read_to_string(p).map(|t| t.trim().to_string()), None => Ok(s) }).transpose()?;
            let input = input.map(|s| client::script::Script::parse(&s, login.as_ref())).transpose()?;
            let drive = client::Drive { login, character, walk_secs: walk, goto, screenshot, screenshot_after: after, quit, input, chase, ..Default::default() };
            client::run(config::Config::load(), drive)
        }
        Cmd::Update { dry_run, data_only, connections, file, to } => {
            if let Some(f) = file {
                let dir = to.unwrap_or_else(|| PathBuf::from("."));
                let (out, stamp) = patch::fetch_one(&f, &dir)?;
                println!("wrote {} (version {stamp})", out.display());
                return Ok(());
            }
            let root = config::Config::load().ffxi_dir();
            println!("checking {} against {}:{} ...", root.display(), patch::HOST, patch::PORT);
            let plan = patch::plan(&root, !data_only)?;
            let local = std::fs::read_to_string(root.join(patch::STAMP_FILE)).ok().and_then(|t| patch::parse_manifest(&t).ok()).and_then(|m| m.iter().flat_map(|e| e.versions.iter().map(|v| v.stamp.clone())).max());
            let bytes: u64 = plan.todo.iter().map(|t| t.2).sum();
            println!("server version {} ({}), last applied here {}", plan.server.version, plan.server.label, local.as_deref().unwrap_or("none"));
            println!("{} files listed: {} current, {} to update ({:.1} MB to download), {} code files left as they are", plan.entries.len(), plan.current, plan.todo.len(), bytes as f64 / 1e6, plan.skipped_code);
            for (i, a, n) in plan.todo.iter().take(20) {
                println!("  {:?} {} ({n} bytes)", a, plan.entries[*i].path);
            }
            if !dry_run && !plan.todo.is_empty() {
                let total = plan.todo.len();
                patch::apply(&root, &plan, connections, &|f, b| {
                    if f % 50 == 0 || f == total {
                        println!("  {f} / {total} files, {:.1} MB", b as f64 / 1e6);
                    }
                })?;
                println!("updated {total} files to {}", plan.server.version);
            }
            Ok(())
        }
        Cmd::Install { source, game_dir, staging, no_verify, keep_staging } => {
            let mut cfg = config::Config::load();
            if let Some(dir) = game_dir {
                cfg.game_dir = dir;
            }
            let opts = install::Options {
                source,
                staging: staging.unwrap_or_else(|| cfg.game_dir.join(".staging")),
                game_dir: cfg.game_dir.clone(),
                verify: !no_verify,
                keep_staging,
            };
            run_with_progress(&opts)?;
            cfg.installed = true;
            cfg.save()?;
            println!("Installed to {}", cfg.ffxi_dir().display());
            Ok(())
        }
        Cmd::DumpMsi { msi, out } => dump_msi(&msi, &out),
        Cmd::CharShot { race, face, head, body, hands, legs, feet, main, sub, range, anim, frame, yaw, motions, out } => {
            use dat::character::{Look, load_character};
            let index = dat::DatIndex::open(&config::Config::load().ffxi_dir())?;
            let mut ch = load_character(&index, &Look { race, face, head, body, hands, legs, feet, main, sub, range })?;
            println!("pc motion info: {:?}", ch.pc);
            for id in motions {
                let f = dat::motionfile::MotionFile::load(&index, id)?;
                println!("motion file {id}: {} animations, schedulers {}", f.animations.len(), f.schedulers.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(" "));
                ch.animations.splice(0..0, f.animations);
            }
            for l in &ch.log {
                println!("{l}");
            }
            let anims: Vec<_> = match &anim {
                None => Vec::new(),
                Some(a) => {
                    let found: Vec<_> = match a.parse::<usize>() {
                        Ok(i) => ch.animations.get(i).into_iter().collect(),
                        Err(_) => ch.find_animations(a),
                    };
                    if found.is_empty() {
                        let names: Vec<&str> = ch.animations.iter().map(|x| x.name.as_str()).collect();
                        anyhow::bail!("no animation {a}; have {}", names.join(" "));
                    }
                    found.into_iter().map(|x| (x, frame.min(x.frames as f32 - 1.0))).collect()
                }
            };
            for (a, f) in &anims {
                println!("animation {} frame {f} of {} ({} tracks, speed {})", a.name, a.frames, a.tracks.len(), a.speed);
            }
            let mesh = render::character::pose_character(&ch, &anims);
            let centre = (mesh.min + mesh.max) / 2.0;
            let size = (mesh.max - mesh.min).max_element();
            let (s, c) = yaw.to_radians().sin_cos();
            let eye = centre + glam::Vec3::new(s, 0.0, c) * size * 1.6;
            let camera = render::Camera { eye, target: centre, up: glam::Vec3::NEG_Y, fov_y: 40f32.to_radians() };
            render::character::render_png(&ch, &mesh, &camera, 600, 800, &out)?;
            println!("bounds {} .. {}; wrote {}", mesh.min, mesh.max, out.display());
            Ok(())
        }
        Cmd::CreatorShot { screen: true, race, face, hair, frame, view, width, height, step, size, job, nation, name, help_time, bench, out, .. } => {
            client::creator_screen_shot(&client::CreatorScreenShot { race, face, hair, size, job, nation, step, name, frame, view, help_time, width, height, bench }, &out)
        }
        Cmd::CreatorShot { race, face, hair, frame, view, width, height, transparent, backdrop, flat, out, .. } => {
            let index = dat::DatIndex::open(&config::Config::load().ffxi_dir())?;
            creatorshot::render(&index, &creatorshot::Shot { race, face, hair, frame, view, width, height, transparent, backdrop, flat }, &out)
        }
        Cmd::EventDump { zone, actor, event } => {
            let index = dat::DatIndex::open(&config::Config::load().ffxi_dir())?;
            eventdump::run(&index, zone, actor, event.map(|e| e as u16))
        }
        Cmd::SoundDump { id, music, file, out, seconds } => sounddump::dump(id, music, file, &out, seconds),
        Cmd::AudioTest { id, seconds, se, fade, device, list } => sounddump::audio_test(id, seconds, se, fade, device, list),
        Cmd::EventSweep { zones } => event_vm::sweep(&zones),
        Cmd::EventRun { zone, event, target, choice, param, race } => {
            let mut p = [0u32; 8];
            p.iter_mut().zip(param).for_each(|(d, s)| *d = s);
            let c: Vec<Option<u32>> = choice.iter().map(|&c| (c >= 0).then_some(c as u32)).collect();
            event_vm::run_cli(zone, event as u16, target, p, &c, race)
        }
        Cmd::ItemDump { lang, key, ids } => itemshot::dump(&lang, key, &ids),
        Cmd::ItemIconShot { ids, out, lang } => itemshot::icon_shot(&ids, &out, &lang),
        Cmd::MsgshiftGen { lsb, out } => {
            let index = dat::DatIndex::open(&config::Config::load().ffxi_dir())?;
            dat::msgshift::generate(&index, &lsb, &out)
        }
        Cmd::MsgshiftMap { zone, ids } => {
            let index = dat::DatIndex::open(&config::Config::load().ffxi_dir())?;
            let shift = dat::msgshift::MsgShift::bundled().verified(&index);
            let dialog = dat::dialog::Dialog::load(&index, zone).ok();
            match shift.zones.get(&zone) {
                Some(z) => println!("zone {zone}: {} segments, {} confidence", z.segs.len(), z.confidence),
                None => println!("zone {zone}: no mapping for this install"),
            }
            for id in ids {
                let to = shift.map_near(zone, id);
                let text = to.and_then(|t| dialog.as_ref().and_then(|d| d.get(t))).unwrap_or_default();
                println!("{zone} {id} -> {} {text}", to.map_or("unknown".into(), |t| t.to_string()));
            }
            Ok(())
        }
        Cmd::EventOps => {
            eventdump::ops();
            Ok(())
        }
        Cmd::CsShot { zone, file, scheduler, out_dir, steps, width, height } => eventdump::cs_shot(zone, file, &scheduler, steps, &out_dir, width, height),
        Cmd::NpcShot { model, look, anim, frame, yaw, motions, out } => npc_shot(model, look, &anim, frame, yaw, &motions, &out),
        Cmd::TextShot { text, font, lang, color, bg, alpha, zoom, out } => text_shot(&text, &font, &lang, &color, &bg, alpha, zoom, &out),
        Cmd::FxShot { zone, out, eye, caster, victim, category, anim, frames, every, hour, dust, file, walk } => fx_shot(zone, &out, &eye, &caster, &victim, (category, anim, dust, file), &walk, frames, every, hour),
        Cmd::FxSweep { out, kinds, ticks, mob_models, only } => fx_sweep(&out, &kinds, ticks, mob_models.as_deref(), &only),
        Cmd::ZoneShot { zone, out, eye, target, hour, weather, effects, moon, start, sched, sub, voyage } => {
            use glam::Vec3;
            let cfg = config::Config::load();
            let index = dat::DatIndex::open(&cfg.ffxi_dir())?;
            let keys = dat::zone::Keys::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?)?;
            let file = index.read(dat::zone::file_id(zone))?;
            let scene = dat::zone::load_scene(&keys, &file)?;
            let lighting = match hour {
                Some(h) => {
                    let w = dat::weather::ZoneWeather::parse(&file)?;
                    let env = w.at(&weather, h * 60.0).ok_or_else(|| anyhow::anyhow!("zone {zone} has no weat keys"))?;
                    println!("weather {weather} at {h:.2} h: {env:?}");
                    Some(dat::weather::Lighting::from_env(&env, h * 60.0))
                }
                None => None,
            };
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for p in &scene.placements {
                lo = lo.min(Vec3::from(p.translate));
                hi = hi.max(Vec3::from(p.translate));
            }
            let centre = (lo + hi) / 2.0;
            if let Some(near) = std::env::var("FFXI_NEAR").ok().and_then(|s| s.split_once(',').and_then(|(x, z)| Some((x.parse::<f32>().ok()?, z.parse::<f32>().ok()?)))) {
                println!("placements {} from {lo} to {hi}", scene.placements.len());
                for p in scene.placements.iter().filter(|p| (p.translate[0] - near.0).abs() < 80.0 && (p.translate[2] - near.1).abs() < 80.0).take(30) {
                    println!("  {} at {:?}", p.name, p.translate);
                }
            }
            let v3 = |v: Vec<f32>| Vec3::new(v[0], v[1], v[2]);
            let (eye, target, up) = match (eye, target) {
                (Some(e), Some(t)) => (v3(e), v3(t), Vec3::NEG_Y),
                _ => (centre + Vec3::new(0.0, -(hi - lo).max_element() * 0.9, 0.01), centre, Vec3::Z),
            };
            let camera = render::Camera { eye, target, up, fov_y: 60f32.to_radians() };
            let sub_file = sub.map(|id| index.read(dat::zone::file_id(id)).map(|f| (id, f))).transpose()?;
                    let sub_scene = sub_file.as_ref().map(|(id, f)| dat::zone::load_scene(&keys, f).map(|s| (*id, s))).transpose()?;
                    let weathers: Vec<dat::weather::ZoneWeather> = sub_file.iter().map(|(_, f)| f).chain([&file]).filter_map(|f| dat::weather::ZoneWeather::parse(f).ok()).collect();
                    // --voyage T[r]: V of the zone's first path at T.
                    let voyage_v = voyage.as_deref().and_then(|v| {
                        let (t, rev) = v.strip_suffix('r').map_or((v, false), |t| (t, true));
                        let m = fx::voyage::Path::parse(scene.paths[0].as_deref()?)?.matrix(t.parse().ok()?, rev);
                        println!("voyage t {t} reversed {rev}: {m}");
                        Some(m)
                    });
                    let setup = |gpu: &render::Gpu, r: &mut render::ZoneRenderer| {
                        if let Some(v) = voyage_v {
                            r.voyage = v;
                        }
                        if let Some((id, s)) = &sub_scene {
                            let mut all = scene.light_keys.clone();
                            all.extend(s.light_keys.iter().filter(|k| **k != 0 && !scene.light_keys.contains(k)));
                            let geo = render::ZoneGeometry::build_keyed(s, &all, r.area_keys());
                            r.set_sub(gpu, *id, &geo, s.textures.clone(), all);
                        }
                        // Area lighting at the same hour and weather, the sub-area's areas first.
                        if let Some(h) = hour.filter(|_| std::env::var_os("FFXI_NO_AREAS").is_none()) {
                            let env = |k: &u32| weathers.iter().find_map(|w| w.areas.get(k)).and_then(|a| a.at(&weather, h * 60.0));
                            let lights = r.area_keys().iter().map(|k| env(k).map(|e| dat::weather::Lighting::from_env(&e, h * 60.0))).collect();
                            println!("areas {:?}", r.area_keys().iter().map(|k| String::from_utf8_lossy(&k.to_le_bytes()).into_owned()).collect::<Vec<_>>());
                            r.set_area_lights(lights);
                        }
                        Vec::new()
                    };
            let draws = match effects {
                None => render::render_pngs_lit(&scene, &[(&camera, out.as_path(), lighting)], 1600, 900, &setup)?,
                Some(ticks) => {
                    let mut fx = fx::Effects::new();
                    fx.set_keys(std::sync::Arc::new(keys));
                    fx.set_global(fx::lib::Library::load(0, index.read(0)?)?);
                    fx.load_zone(fx::lib::Library::load(dat::zone::file_id(zone), file.clone())?);
                    if let Some((id, f)) = &sub_file {
                        fx.load_sub_area(fx::lib::Library::load(dat::zone::file_id(*id), f.clone())?);
                    }
                    fx.set_weather(&weather, 0.0);
                    // Names under 4 letters are NUL padded ("c37" is c37 NUL).
                    let fourcc = |s: &str| (s.len() <= 4).then(|| std::array::from_fn::<u8, 4, _>(|i| s.as_bytes().get(i).copied().unwrap_or(0)));
                    if let Some(name) = start.as_deref().and_then(fourcc)
                        && let Some(l) = fx.reg.lib_of(dat::zone::file_id(zone))
                        && let Some(n) = fx.reg.libs[l].find_global(dat::generator::kind::GENERATOR, u32::from_le_bytes(name))
                    {
                        fx.start((l, n), fx::Owner::Zone);
                    }
                    let sched = sched.as_deref().and_then(|s| s.split_once(':')).and_then(|(f, t)| Some((f.parse::<u32>().ok()?, u32::from_le_bytes(<[u8; 4]>::try_from(t.as_bytes()).ok()?))));
                    if let Some((file, name)) = sched {
                        fx.set_index(std::sync::Arc::new(dat::DatIndex::open(&config::Config::load().ffxi_dir())?));
                        fx.preload(&[file]);
                        fx.on_event_scheduler(file, name, fx::sched::Who::None, fx::sched::Who::None);
                    }
                    let dayfrac = hour.unwrap_or(12.0) / 24.0;
                    // One renderer for both frames: the zone's meshes and textures upload once.
                    let mut eg_keep: Option<render::effects::EffectsGpu> = None;
                    let mut post = |gpu: &render::Gpu, enc: &mut wgpu::CommandEncoder, (color, color_tex): (&wgpu::TextureView, &wgpu::Texture), depth: &wgpu::TextureView, cam: &render::Camera, l: Option<&dat::weather::Lighting>, zr: &render::ZoneRenderer| {
                        let eg = eg_keep.get_or_insert_with(|| render::effects::EffectsGpu::new(gpu, wgpu::TextureFormat::Rgba8Unorm));
                        eg.set_lights(&zr.lights);
                        eg.set_light(l.map(|l| (l.dir, l.diffuse, l.ambient)));
                        fx.reg.light_keys = zr.light_keys.clone();
                        let view = cam.view();
                        let mut view_rot = view;
                        view_rot.w_axis = glam::Vec4::W;
                        let f = fx::Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac, view, view_rot, eye: cam.eye, draw_distance: 2000.0, area_light: l.map_or(glam::Vec3::ONE, |l| l.area), focal: 350.0, moon_phase: moon, weekday: 0 };
                        for _ in 0..ticks {
                            fx.tick(&f, &fx::NoActors);
                            let mut e = gpu.device.create_command_encoder(&Default::default());
                            eg.update(gpu, &mut e, &mut fx, &f);
                            gpu.queue.submit([e.finish()]);
                            eg.after_submit(gpu, &mut fx);
                            let _ = gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
                            eg.after_submit(gpu, &mut fx);
                        }
                        println!("effects: {} live elements after {ticks} ticks", fx.live_count());
                        if std::env::var_os("FFXI_FX_LIST").is_some() {
                            for l in fx.describe() {
                                println!("  {l}");
                            }
                            if let Ok(id) = std::env::var("FFXI_FX_TEX").map(|s| s.parse::<usize>().unwrap_or(0)) {
                                let r = fx.reg.sprites[id];
                                if let Some(t) = fx.reg.libs[r.0].sprites.get(&r.1).and_then(|s| s.texture) {
                                    let tex = &fx.reg.libs[t.0].textures[t.1];
                                    let alphas: std::collections::BTreeSet<u8> = tex.rgba.chunks(4).map(|p| p[3]).collect();
                                    println!("  sprite {id} texture {} {}x{} format {} dxt {:?} alphas {:?}", tex.name, tex.width, tex.height, tex.format, tex.dxt.as_ref().map(|d| d.0), alphas);
                                    if let Ok(f) = std::fs::File::create(out.with_extension("tex.png")) {
                                        let mut enc = png::Encoder::new(std::io::BufWriter::new(f), tex.width, tex.height);
                                        enc.set_color(png::ColorType::Rgba);
                                        let _ = enc.write_header().and_then(|mut w| w.write_image_data(&tex.rgba));
                                    }
                                }
                            }
                            if std::env::var_os("FFXI_FX_MESH").is_some() {
                                let mut seen = std::collections::BTreeSet::new();
                                for c in fx.elems.iter().flatten() {
                                    if let fx::Draw::Mesh(id) = c.draw
                                        && seen.insert(id)
                                    {
                                        let r = fx.reg.meshes[id as usize];
                                        if let Some(m) = fx.reg.libs[r.0].meshes.get(&r.1) {
                                            let g: Vec<String> = m.groups.iter().map(|g| format!("{:?}->{:?} {}", g.name.as_deref().map(str::trim), g.texture, g.range.len())).collect();
                                            println!("  mesh {id} {}:{} mmb {} {}", fx.reg.libs[r.0].file_id, fx.reg.libs[r.0].tree.nodes[r.1].name(), m.mmb, g.join(", "));
                                        }
                                    }
                                }
                            }
                            if let Ok(want) = std::env::var("FFXI_FX_OPS") {
                                for inst in fx.insts.iter().flatten().filter(|x| x.name == want.as_bytes()[..4]).take(1) {
                                    for (k, s) in inst.streams.iter().enumerate() {
                                        for op in s {
                                            let w: Vec<String> = op.words.iter().skip(1).map(|w| format!("{w:#x}={:.3}", f32::from_bits(*w))).collect();
                                            println!("  stream {k} op {:#04x} slot {} {}", op.code, op.slot, w.join(" "));
                                        }
                                    }
                                }
                            }
                        }
                        let (fog_color, fog) = l.map_or(([0.0; 3], [150.0, 600.0]), |l| (l.fog.to_array(), [l.fog_start, l.fog_end]));
                        eg.draw(gpu, enc, (color, depth, [1600, 900]), Some(color_tex), &fx, (cam.proj(1600.0 / 900.0), cam.view(), cam.eye), [crate::render::NEAR, crate::render::FAR], fog_color, fog);
                    };
                    // Twice: the effects write the point lights the scenery reads on the next frame.
                    render::render_pngs_post(&scene, &[(&camera, out.as_path(), lighting), (&camera, out.as_path(), lighting)], 1600, 900, &setup, &mut post)?
                }
            };
            println!("rendered {draws} draws from {eye} towards {target} into {}", out.display());
            Ok(())
        }
        Cmd::MmbDump { zone, name } => {
            let cfg = config::Config::load();
            let index = dat::DatIndex::open(&cfg.ffxi_dir())?;
            let keys = dat::zone::Keys::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?)?;
            let file = index.read(dat::zone::file_id(zone))?;
            for c in dat::chunks(&file)?.into_iter().filter(|c| c.kind == 0x2E) {
                let mut data = c.data.to_vec();
                dat::zone::decode_mmb(&keys, &mut data);
                let Ok(mesh) = dat::zone::parse_mmb(&data) else { continue };
                if mesh.name != name {
                    continue;
                }
                let u32_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
                println!("chunk {:#x} len {} head {:02x?} pieces {} first {:#x} bounds {:?}..{:?}", c.offset, data.len(), &data[..16], u32_at(32), u32_at(60), mesh.min, mesh.max);
                println!("offset table: {:x?}", (0..8).map(|i| u32_at(64 + i * 4)).collect::<Vec<_>>());
                for (i, m) in mesh.models.iter().enumerate() {
                    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                    for v in &m.vertices {
                        for k in 0..3 {
                            lo[k] = lo[k].min(v.pos[k]);
                            hi[k] = hi[k].max(v.pos[k]);
                        }
                    }
                    println!("  model {i}: tex {:<16} verts {:4} idx {:4} strip {} bounds {:?}..{:?}", m.texture, m.vertices.len(), m.indices.len(), m.strip, lo, hi);
                }
            }
            Ok(())
        }
        Cmd::ZonePath { zone, from, to } => zone_path(zone, &from, &to),
        Cmd::ZoneProbe { zone, x, z } => {
            use glam::{Mat4, Vec3};
            let cfg = config::Config::load();
            let index = dat::DatIndex::open(&cfg.ffxi_dir())?;
            let keys = dat::zone::Keys::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?)?;
            let scene = dat::zone::load_scene(&keys, &index.read(dat::zone::file_id(zone))?)?;
            let textures: std::collections::HashMap<&str, &dat::zone::Texture> = scene.textures.iter().map(|t| (t.name.as_str(), t)).collect();
            for p in &scene.placements {
                let Some(mesh) = scene.meshes.iter().find(|m| m.name == p.name) else { continue };
                let m = Mat4::from_cols_array_2d(&p.matrix());
                for (mi, model) in mesh.models.iter().enumerate() {
                    let w: Vec<Vec3> = model.vertices.iter().map(|v| m.transform_point3(Vec3::from(v.pos))).collect();
                    let tris: Vec<[u16; 3]> = if model.strip {
                        model.indices.windows(3).map(|t| [t[0], t[1], t[2]]).collect()
                    } else {
                        model.indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect()
                    };
                    let hit = tris.iter().find_map(|t| {
                        let [a, b, c] = t.map(|i| w[i as usize]);
                        let d = (b.x - a.x) * (c.z - a.z) - (c.x - a.x) * (b.z - a.z);
                        if d.abs() < 1e-8 {
                            return None;
                        }
                        let u = ((x - a.x) * (c.z - a.z) - (c.x - a.x) * (z - a.z)) / d;
                        let v = ((b.x - a.x) * (z - a.z) - (x - a.x) * (b.z - a.z)) / d;
                        (u >= 0.0 && v >= 0.0 && u + v <= 1.0).then(|| a.y + u * (b.y - a.y) + v * (c.y - a.y))
                    });
                    let Some(y) = hit else { continue };
                    let va = model.vertices.iter().map(|v| (v.color >> 24) as f32).sum::<f32>() / model.vertices.len() as f32;
                    let tex = textures.get(model.texture.as_str()).map(|t| {
                        let a = t.rgba.chunks_exact(4).map(|p| p[3] as f32).sum::<f32>() / (t.rgba.len() / 4) as f32;
                        format!("fmt {:#04x} {}x{} texel alpha {:.0}", t.format, t.width, t.height, a)
                    });
                    println!(
                        "y {:7.2}  {} model {} tex {:<16} {}  verts {} strip {} blend {:#06x} vertex alpha {:.0} scale {:?}",
                        y, p.name, mi, model.texture, tex.unwrap_or_else(|| "MISSING".into()), model.vertices.len(), model.strip, model.blending, va, p.scale
                    );
                }
            }
            Ok(())
        }
        Cmd::ZoneInfo { zone } => {
            let cfg = config::Config::load();
            let index = dat::DatIndex::open(&cfg.ffxi_dir())?;
            let keys = dat::zone::Keys::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?)?;
            let scene = dat::zone::load_scene(&keys, &index.read(dat::zone::file_id(zone))?)?;
            let names: std::collections::HashSet<&str> = scene.meshes.iter().map(|m| m.name.as_str()).collect();
            let tex: std::collections::HashSet<&str> = scene.textures.iter().map(|t| t.name.as_str()).collect();
            let placed = scene.placements.iter().filter(|p| names.contains(p.name.as_str())).count();
            let models: Vec<_> = scene.meshes.iter().flat_map(|m| &m.models).collect();
            let textured = models.iter().filter(|m| tex.contains(m.texture.as_str())).count();
            let (verts, idx): (usize, usize) = models.iter().fold((0, 0), |a, m| (a.0 + m.vertices.len(), a.1 + m.indices.len()));
            let bad_idx = models.iter().filter(|m| m.indices.iter().any(|&i| i as usize >= m.vertices.len())).count();
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for p in &scene.placements {
                for k in 0..3 {
                    lo[k] = lo[k].min(p.translate[k]);
                    hi[k] = hi[k].max(p.translate[k]);
                }
            }
            println!("placements {} ({} resolve to a mesh), meshes {}, models {} ({} with their texture), textures {}", scene.placements.len(), placed, scene.meshes.len(), models.len(), textured, scene.textures.len());
            println!("vertices {verts}, indices {idx}, models with out-of-range indices {bad_idx}");
            for r in scene.regions.iter().filter(|r| r.name.starts_with(['z', 'Z'])) {
                println!("zone line {:<8} at ({:.2}, {:.2}, {:.2}) rot {:.2} size {:?}", r.name, r.pos[0], r.pos[1], r.pos[2], r.rot[1], r.scale);
            }
            println!("{} RID regions", scene.regions.len());
            let mut areas: std::collections::BTreeMap<String, (usize, glam::Vec3)> = Default::default();
            for p in scene.placements.iter().filter(|p| p.area != 0) {
                let e = areas.entry(String::from_utf8_lossy(&p.area.to_le_bytes()).into_owned()).or_insert((0, glam::Vec3::from(p.translate)));
                e.0 += 1;
            }
            for (a, (n, at)) in &areas {
                println!("area {a}: {n} placements, e.g. at {at}");
            }
            if let Some(c) = &scene.collision {
                let (mut lo, mut hi) = (glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN));
                for t in &c.tris {
                    for v in t {
                        lo = lo.min(*v);
                        hi = hi.max(*v);
                    }
                }
                let flags: std::collections::BTreeSet<u16> = c.flags.iter().copied().collect();
                println!("collision: grid {:?}, {} triangles, bounds {lo} .. {hi}, flags {flags:x?}", c.grid, c.tris.len());
            }
            println!("placement bounds {lo:?} .. {hi:?}");
            for p in scene.placements.iter().take(3) {
                println!("  {:?}", p);
            }
            // Alpha profile per texture format: how much would an alpha test at 0.3 discard?
            let mut by_format: std::collections::BTreeMap<u8, (usize, f64, f64)> = Default::default();
            for t in &scene.textures {
                let a: Vec<u8> = t.rgba.chunks_exact(4).map(|p| p[3]).collect();
                let mean = a.iter().map(|&x| x as f64).sum::<f64>() / a.len() as f64;
                let low = a.iter().filter(|&&x| x < 77).count() as f64 / a.len() as f64;
                let e = by_format.entry(t.format).or_default();
                e.0 += 1;
                e.1 += mean;
                e.2 += low;
            }
            for (f, (n, mean, low)) in &by_format {
                println!("IMG format {f:#04x}: {n} textures, mean alpha {:.0}, {:.0}% of texels under 77", mean / *n as f64, low / *n as f64 * 100.0);
            }
            let mut vcol = [0u64; 4];
            let mut vn = 0u64;
            for m in &models {
                for v in &m.vertices {
                    for (k, b) in v.color.to_le_bytes().iter().enumerate() {
                        vcol[k] += *b as u64;
                    }
                    vn += 1;
                }
            }
            println!("mean vertex colour BGRA {:?}", vcol.map(|c| c / vn.max(1)));
            for w in scene.warnings.iter().take(10) {
                println!("warning: {w}");
            }
            println!("{} warnings", scene.warnings.len());
            Ok(())
        }
        Cmd::UiDump { id, out } => uishot::ui_dump(id, &out),
        Cmd::TitleShot { out, width, height } => uishot::title_shot(&out, width, height),
        Cmd::UiShot { out, width, height } => uishot::ui_shot(&out, width, height),
        Cmd::TopbarShot { out, fade, menu, menu_swap, target, hpp, map, downloading, window, config, cfgset, installed, width, height } => client::topbar_shot(&out, &fade, menu, menu_swap.as_deref(), target.as_deref(), hpp, map.as_deref(), downloading, window.as_deref(), config.as_deref(), cfgset.as_deref(), installed, width, height),
        Cmd::FlashShot { zone, hour, pos, out_dir } => {
            let v: Vec<f32> = pos.split(',').filter_map(|s| s.trim().parse().ok()).collect();
            anyhow::ensure!(v.len() == 3, "--pos wants x,y,z");
            flashshot::run(zone, hour, [v[0], v[1], v[2]], &out_dir)
        }
        Cmd::CombatShot { out, mob, layout, width, height } => combatshot::combat_shot(&out, &mob, layout, width, height),
        Cmd::InventoryShot { out, scroll, cursor, frame, width, height } => invshot::inventory_shot(&out, scroll, cursor, frame, width, height),
        Cmd::EquipmentShot { out, cursor, frame, width, height } => invshot::equipment_shot(&out, cursor, frame, width, height),
        Cmd::DialogueShot { out, zone, id, speaker, choice, frame, width, height } => dialogshot::dialogue_shot(&out, zone, id, &speaker, choice, frame, width, height),
        Cmd::FlyoverShot { scheduler, out_dir, steps, flat, width, height } => flyover_shot(&scheduler, &out_dir, steps, flat, width, height),
        Cmd::SelectShot { out, slot, chars, hold, frames, close, ui, delete, rename, unusable, zone, hour, width, height } => {
            let frames = if close { frames.max(60) } else { frames };
            let shot = client::select::ScreenShot { chars, cursor: slot, hold, frames, ui, deleting: delete, rename, unusable, zone, hour, width, height };
            client::select::screen_shot(&shot, &out)
        }
        Cmd::MsgDump { group, ids } => {
            let cfg = config::Config::load();
            let index = dat::DatIndex::open(&cfg.ffxi_dir())?;
            let dll = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?;
            let file = dat::msg::group_file(&dll, 1, group).ok_or_else(|| anyhow::anyhow!("no file for group {group:#x}"))?;
            let m = dat::msg::Messages::load(&index, file)?;
            println!("group {group:#x}: file {file}");
            let ids = if ids.is_empty() { (0..4096).take_while(|&i| m.raw(i).is_some()).collect() } else { ids };
            for i in ids {
                println!("{i:#x}: {:?}", m.get(i).unwrap_or_default());
            }
            Ok(())
        }
        Cmd::DatInfo { id, all } => {
            let index = dat::DatIndex::open(&config::Config::load().ffxi_dir())?;
            let file = index.read(id)?;
            println!("{} ({} bytes, index of {} ids)", index.path(id).unwrap().display(), file.len(), index.len());
            let chunks = dat::chunks(&file)?;
            let mut kinds: std::collections::BTreeMap<u8, usize> = Default::default();
            for c in &chunks {
                *kinds.entry(c.kind).or_default() += 1;
            }
            println!("{} chunks; by type: {:?}", chunks.len(), kinds);
            for c in chunks.iter().filter(|c| c.kind != 0 && c.kind != 1).take(if all { usize::MAX } else { 12 }) {
                println!("  {:#08x} {:<4} type {:#04x} {} bytes", c.offset, c.name(), c.kind, c.data.len());
            }
            Ok(())
        }
        Cmd::AccountCreate { credentials, user } => {
            use net::auth::{self, Command, Request};
            let cfg = config::Config::load();
            let creds = Credentials { username: user, password: random_password()? };
            let req = Request { username: &creds.username, password: &creds.password, ..Default::default() };
            auth::send(&cfg.server, cfg.auth_port, Command::CreateAccount, &req)?;
            write_private(&credentials, &serde_json::to_vec_pretty(&creds)?)?;
            println!("Created account {}; credentials in {}", creds.username, credentials.display());
            Ok(())
        }
        Cmd::Login { credentials, create_char, race, face, size, job, nation, select, zone_in: stay } => {
            let cfg = config::Config::load();
            let creds: Credentials = serde_json::from_slice(&std::fs::read(&credentials)?)?;
            let mut lobby = login(&cfg, &creds)?;
            let mut chars = lobby.characters()?;
            if let Some(name) = &create_char {
                lobby.create_character(&net::lobby::NewCharacter { name, race, face, size, job, nation })?;
                println!("Created {name}");
                chars = lobby.characters()?;
            }
            for w in lobby.worlds()? {
                println!("World 0x{:02X}: {}", w.0, w.1);
            }
            for c in chars.iter().filter(|c| !c.is_empty_slot()) {
                println!(
                    "  {:<15} id {:>6}  race {} job {}{} lv{} zone {} nation {} looks {:?}",
                    c.name, c.content_id, c.race, c.main_job, if c.sub_job > 0 { format!("/{}", c.sub_job) } else { String::new() },
                    c.main_level, c.zone, c.nation, c.looks
                );
            }
            println!("  ({} empty slots)", chars.iter().filter(|c| c.is_empty_slot()).count());
            if let Some(name) = select {
                let ch = chars.iter().find(|c| c.name.eq_ignore_ascii_case(&name)).ok_or_else(|| anyhow::anyhow!("no character {name}"))?;
                let m = lobby.select(ch)?;
                println!("Map server {}:{}, search {}:{}, char {} ({}), key {}", m.map.0, m.map.1, m.search.0, m.search.1, m.char_id, m.name, hex::encode(m.key));
                if let Some(secs) = stay {
                    zone_in(&cfg, &m, &creds.username, secs)?;
                }
            }
            Ok(())
        }
    }
}

fn npc_shot(model: Option<u16>, look: Option<String>, anim: &str, frame: f32, yaw: f32, motions: &[u32], out: &std::path::Path) -> Result<()> {
    use dat::npc::{NpcLook, load_npc, parse_look};
    let look = match (model, look) {
        (Some(m), None) => NpcLook::Model(m),
        (None, Some(h)) => {
            let h = h.trim_start_matches("0x");
            let bytes = (0..h.len() / 2).map(|i| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16)).collect::<Result<Vec<_>, _>>()?;
            parse_look(&bytes)?
        }
        _ => anyhow::bail!("give exactly one of --model and --look"),
    };
    println!("{look:?}");
    let index = dat::DatIndex::open(&config::Config::load().ffxi_dir())?;
    let mut ch = load_npc(&index, &look)?;
    for &id in motions {
        let f = dat::motionfile::MotionFile::load(&index, id)?;
        println!("motion file {id}: {} animations, schedulers {}", f.animations.len(), f.schedulers.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(" "));
        ch.animations.splice(0..0, f.animations);
    }
    for l in &ch.log {
        println!("{l}");
    }
    let found: Vec<_> = match anim.parse::<usize>() {
        Ok(i) => ch.animations.get(i).into_iter().collect(),
        Err(_) if anim.contains(',') => anim.split(',').flat_map(|a| ch.animations.iter().filter(move |x| x.name.trim_end() == a)).collect(),
        Err(_) => ch.find_animations(anim),
    };
    if found.is_empty() {
        let names: Vec<&str> = ch.animations.iter().map(|x| x.name.as_str()).collect();
        println!("no animation {anim}; rest pose. Have: {}", names.join(" "));
    }
    let anims: Vec<_> = found.into_iter().map(|x| (x, frame.min(x.frames as f32 - 1.0))).collect();
    for (a, f) in &anims {
        println!("animation {} frame {f} of {} ({} tracks, speed {})", a.name, a.frames, a.tracks.len(), a.speed);
    }
    let mesh = render::character::pose_character(&ch, &anims);
    let centre = (mesh.min + mesh.max) / 2.0;
    let size = (mesh.max - mesh.min).max_element();
    let (s, c) = yaw.to_radians().sin_cos();
    let eye = centre + glam::Vec3::new(s, 0.0, c) * size * 1.6;
    let camera = render::Camera { eye, target: centre, up: glam::Vec3::NEG_Y, fov_y: 40f32.to_radians() };
    render::character::render_png(&ch, &mesh, &camera, 600, 800, out)?;
    println!("bounds {} .. {}; wrote {}", mesh.min, mesh.max, out.display());
    Ok(())
}

fn run_with_progress(opts: &install::Options) -> Result<()> {
    let progress = Arc::new(install::Progress::default());
    std::thread::scope(|s| {
        let worker = s.spawn(|| install::install(opts, progress.clone()));
        while !worker.is_finished() {
            std::thread::sleep(Duration::from_secs(2));
            let gib = |n: u64| n as f64 / (1u64 << 30) as f64;
            eprintln!(
                "{}: {:.1}% ({:.2}/{:.2} GiB)",
                progress.phase.lock().unwrap(),
                progress.fraction() * 100.0,
                gib(progress.done.load(Ordering::Relaxed)),
                gib(progress.total.load(Ordering::Relaxed)),
            );
        }
        worker.join().unwrap()
    })
}

fn dump_msi(path: &PathBuf, out: &PathBuf) -> Result<()> {
    let mut pkg = msi::open(path)?;
    std::fs::create_dir_all(out)?;
    let names: Vec<String> = pkg.tables().map(|t| t.name().to_string()).collect();
    for name in &names {
        let header: Vec<String> = pkg.get_table(name).unwrap().columns().iter().map(|c| c.name().to_string()).collect();
        let mut text = header.join("\t") + "\n";
        for row in pkg.select_rows(msi::Select::table(name))? {
            let cells: Vec<String> = (0..row.len()).map(|i| row[i].to_string()).collect();
            text += &(cells.join("\t") + "\n");
        }
        std::fs::write(out.join(format!("{name}.tsv")), text)?;
    }
    Ok(())
}

/// UTF-8 to Shift-JIS with "\xHH" escapes for raw bytes.
fn sjis_with_escapes(s: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(at) = rest.find("\\x") {
        out.extend(dat::font::encode_sjis(&rest[..at]));
        let hex = rest.get(at + 2..at + 4).ok_or_else(|| anyhow::anyhow!("short \\x escape"))?;
        out.push(u8::from_str_radix(hex, 16)?);
        rest = &rest[at + 4..];
    }
    out.extend(dat::font::encode_sjis(rest));
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn text_shot(text: &[String], font: &str, lang: &str, color: &str, bg: &str, alpha: bool, zoom: u32, out: &std::path::Path) -> Result<()> {
    use dat::font::{Font, Quality};
    let quality = match font {
        "compressed" | "moji" | "0" => Quality::Compressed,
        "uncompressed" | "1" => Quality::Uncompressed,
        "high" | "2" => Quality::High,
        other => anyhow::bail!("unknown font {other}; use compressed, uncompressed or high"),
    };
    let lang = ["ja", "en", "fr", "de"].iter().position(|&l| l == lang).ok_or_else(|| anyhow::anyhow!("lang must be ja, en, fr or de"))? as u8;
    let hex4 = |s: &str| -> Result<[u8; 4]> { Ok(u32::from_str_radix(s, 16)?.to_be_bytes()) };
    let color = hex4(color)?;
    let bg = hex4(&format!("{bg}00"))?;
    let cfg = config::Config::load();
    let index = dat::DatIndex::open(&cfg.ffxi_dir())?;
    let font = Font::load(&index, &std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?, lang, quality)?;
    println!("atlas {}x{} ({} px cells), {} gaiji shapes, {} shape textures", font.atlas.width, font.atlas.height, font.cell_px, font.gaiji.len(), font.textures.len());
    let mut quads = Vec::new();
    let (pad, mut w, mut h) = (8, 0, 0);
    for line in text {
        let sjis = sjis_with_escapes(line)?;
        let (lw, lh) = font.layout(&sjis, pad, pad + h, color, &mut quads);
        println!("{:<40} sjis {} width {lw} ({})", dat::font::decode_sjis(&sjis), hex::encode(&sjis), font.text_width(&sjis));
        w = w.max(lw);
        h += lh;
    }
    let (width, height) = ((w + pad * 2) as u32, (h + pad * 2) as u32);
    let mut img: Vec<u8> = (0..width * height).flat_map(|_| [bg[0], bg[1], bg[2], 255]).collect();
    font.draw(&quads, &mut img, width, height, alpha);
    let (zw, zh) = (width * zoom, height * zoom);
    let zoomed: Vec<u8> = (0..zh).flat_map(|y| (0..zw).map(move |x| ((y / zoom) * width + x / zoom) as usize)).flat_map(|i| img[i * 4..i * 4 + 4].to_vec()).collect();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(out)?), zw, zh);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header()?.write_image_data(&zoomed)?;
    println!("{} quads; wrote {}x{} to {}", quads.len(), zw, zh, out.display());
    Ok(())
}

fn load_zone_scene(zone: u32) -> Result<dat::zone::ZoneScene> {
    let cfg = config::Config::load();
    let index = dat::DatIndex::open(&cfg.ffxi_dir())?;
    let keys = dat::zone::Keys::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?)?;
    dat::zone::load_scene(&keys, &index.read(dat::zone::file_id(zone))?)
}

fn load_zone_weather(zone: u32) -> Result<dat::weather::ZoneWeather> {
    let index = dat::DatIndex::open(&config::Config::load().ffxi_dir())?;
    dat::weather::ZoneWeather::parse(&index.read(dat::zone::file_id(zone))?)
}

fn flyover_shot(scheduler: &str, out_dir: &std::path::Path, steps: u32, flat: bool, width: u32, height: u32) -> Result<()> {
    use dat::title::{Op, TITLE_FILE, Title, fov_y, hour_of, sample};
    use dat::weather::{Env, Lighting};
    let index = dat::DatIndex::open(&config::Config::load().ffxi_dir())?;
    let file = index.read(TITLE_FILE)?;
    let title = Title::parse(&file)?;
    // Type 0x2f environment blocks of file 23, set by op 0x7e (FUN_10183ff0).
    let envs: Vec<(String, Env)> = dat::chunks(&file)?.into_iter().filter(|c| c.kind == 0x2f).filter_map(|c| Env::parse(c.data).map(|e| (c.name(), e))).collect();
    let ops = title.scheduler(scheduler)?;
    std::fs::create_dir_all(out_dir)?;
    let zone = ops.iter().find_map(|o| if let Op::Zone(z) = o { Some(*z) } else { None }).ok_or_else(|| anyhow::anyhow!("scheduler loads no zone"))?;
    let scene = load_zone_scene(zone)?;
    let weather = load_zone_weather(zone)?;
    // Lobby clock before the title: 12:30 (FUN_1023b3f0(12, 30)).
    let (mut minute, mut tag, mut env_override): (f32, String, Option<Env>) = (12.5 * 60.0, "suny".into(), None);
    let mut frames = Vec::new();
    for op in ops {
        match op {
            Op::Time(t) => {
                minute = hour_of(*t) * 60.0;
                println!("time {t} = {:.2} h", hour_of(*t));
            }
            Op::Env(name) => {
                env_override = envs.iter().find(|e| &e.0 == name).map(|e| e.1);
                println!("env {name} (title 0x2f override)");
            }
            Op::Weather { tag: t, ticks } => {
                // Inferred: a new weather object replaces the override with the zone's own keys.
                tag = t.clone();
                env_override = None;
                println!("weather {t} over {ticks}");
            }
            Op::Camera { path, wait, ticks } => {
                let p = title.path(path)?;
                let env = env_override.or_else(|| weather.at(&tag, minute));
                let lighting = if flat { None } else { env.map(|e| Lighting::from_env(&e, minute)) };
                println!("camera {path}: {} keys, ease {}, {ticks} ticks, next op after {wait}; light {}", p.keys.len(), p.ease, if lighting.is_some() { "retail" } else { "flat" });
                for i in 0..steps.max(1) {
                    let t = if steps <= 1 { 0.0 } else { i as f32 / (steps - 1) as f32 };
                    let k = sample(p, t);
                    let cam = render::Camera { eye: k.eye, target: k.target, up: glam::Vec3::NEG_Y, fov_y: fov_y(k.focal) };
                    frames.push((cam, out_dir.join(format!("{scheduler}_{path}_{i}.png")), lighting));
                }
            }
            _ => {}
        }
    }
    let list: Vec<(&render::Camera, &std::path::Path, Option<Lighting>)> = frames.iter().map(|(c, n, l)| (c, n.as_path(), *l)).collect();
    render::render_pngs_lit(&scene, &list, width, height, &|_, _| Vec::new())?;
    println!("zone {zone}: wrote {} frames to {}", list.len(), out_dir.display());
    Ok(())
}

/// Decimal or 0x-prefixed hex.
fn parse_num(s: &str) -> Result<u32, String> {
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => u32::from_str_radix(h, 16),
        None => s.parse(),
    }
    .map_err(|e| e.to_string())
}

/// A* over 1-unit cells; an edge is walkable when Ground::walk reaches its end.
fn zone_path(zone: u32, from: &str, to: &str) -> Result<()> {
    use glam::Vec3;
    use std::collections::{BinaryHeap, HashMap};
    let nums = |s: &str| -> Result<Vec<f32>> { s.split(',').map(|v| Ok(v.trim().parse::<f32>()?)).collect() };
    let (f, t) = (nums(from)?, nums(to)?);
    anyhow::ensure!(f.len() == 3 && t.len() == 2, "want --from x,y,z --to x,z");
    let cfg = config::Config::load();
    let index = dat::DatIndex::open(&cfg.ffxi_dir())?;
    let keys = dat::zone::Keys::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?)?;
    let ground = client::ground::Ground::build(&dat::zone::load_scene(&keys, &index.read(dat::zone::file_id(zone))?)?);
    let start = Vec3::new(f[0], f[1], f[2]);
    let cell = |x: f32, z: f32| ((x.round()) as i32, (z.round()) as i32);
    let goal = cell(t[0], t[1]);
    let h = |c: (i32, i32)| (((c.0 - goal.0).pow(2) + (c.1 - goal.1).pow(2)) as f32).sqrt();
    let s0 = cell(start.x, start.z);
    let mut pos: HashMap<(i32, i32), Vec3> = HashMap::from([(s0, start)]);
    let mut cost: HashMap<(i32, i32), f32> = HashMap::from([(s0, 0.0)]);
    let mut came: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    let mut open = BinaryHeap::new();
    open.push((std::cmp::Reverse((h(s0) * 100.0) as i64), s0));
    let mut found = None;
    while let Some((_, c)) = open.pop() {
        if c == goal || h(c) < 1.5 {
            found = Some(c);
            break;
        }
        if cost.len() > 400_000 {
            break;
        }
        let p = pos[&c];
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let n = (c.0 + dx, c.1 + dz);
            let want = Vec3::new(n.0 as f32, p.y, n.1 as f32);
            let got = ground.walk(p, want, 1.2);
            if (got.x - want.x).hypot(got.z - want.z) > 0.15 {
                continue;
            }
            let g = cost[&c] + ((dx * dx + dz * dz) as f32).sqrt();
            if cost.get(&n).is_none_or(|&old| g < old) {
                cost.insert(n, g);
                pos.insert(n, got);
                came.insert(n, c);
                open.push((std::cmp::Reverse(((g + h(n)) * 100.0) as i64), n));
            }
        }
    }
    let Some(mut c) = found else { anyhow::bail!("no path ({} cells searched)", cost.len()) };
    let mut cells = vec![c];
    while let Some(&p) = came.get(&c) {
        cells.push(p);
        c = p;
    }
    cells.reverse();
    // Keep a cell only where a straight walk from the last kept one no longer gets through.
    let mut keep = vec![0];
    let mut i = 0;
    while i + 1 < cells.len() {
        let mut j = i + 1;
        while j + 1 < cells.len() {
            let (a, b) = (pos[&cells[i]], pos[&cells[j + 1]]);
            let got = ground.walk(a, b, 1.2);
            if (got.x - b.x).hypot(got.z - b.z) > 0.15 {
                break;
            }
            j += 1;
        }
        keep.push(j);
        i = j;
    }
    let pts: Vec<String> = keep.iter().skip(1).map(|&k| format!("{},{}", cells[k].0, cells[k].1)).collect();
    println!("{}", pts.join(";"));
    Ok(())
}

/// fx-sweep's actors: a Hume M (PC) and a monster facing each other 4 apart, attach points at chest height; the
/// monster's files are the caster model's when one is given.
struct SweepActors {
    mob_files: (Vec<u32>, Vec<u32>),
    fx_files: std::cell::RefCell<Vec<(u32, u32)>>,
}

/// The client's side of op 0x05's effect motion files for the stand-in actors: on while a task owns them.
fn sync_fx_files(fx: &mut fx::Effects, list: &std::cell::RefCell<Vec<(u32, u32)>>) {
    let mut l = list.borrow_mut();
    l.extend(std::mem::take(&mut fx.effect_motions));
    for d in std::mem::take(&mut fx.effect_motions_done) {
        l.retain(|x| *x != d);
    }
}

const SWEEP_PC: u32 = 0x0001_0001;
const SWEEP_MOB: u32 = 0x0100_0001;

impl fx::Actors for SweepActors {
    fn matrix(&self, id: u32) -> Option<glam::Mat4> {
        let (p, f) = if id == SWEEP_PC { (glam::Vec3::ZERO, glam::Vec3::Z) } else { (glam::Vec3::new(0.0, 0.0, 4.0), glam::Vec3::NEG_Z) };
        Some(glam::Mat4::from_cols(f.extend(0.0), glam::Vec3::Y.extend(0.0), f.cross(glam::Vec3::Y).extend(0.0), p.extend(1.0)))
    }
    fn foot(&self, id: u32) -> Option<glam::Vec3> {
        self.matrix(id).map(|m| m.w_axis.truncate())
    }
    fn point(&self, _: u32, k: u16) -> Option<glam::Vec3> {
        Some(if k == 0 { glam::Vec3::ZERO } else { glam::Vec3::new(0.0, -1.2, 0.0) })
    }
    fn scale(&self, _: u32) -> f32 {
        1.0
    }
    fn files(&self, id: u32) -> (Vec<u32>, Vec<u32>) {
        let (p, mut m) = if id == SWEEP_PC { (vec![8392], vec![9672, 7072]) } else { self.mob_files.clone() };
        m.extend(self.fx_files.borrow().iter().filter(|x| x.0 == id).map(|x| x.1));
        (p, m)
    }
    fn hidden(&self, _: u32) -> bool {
        false
    }
    fn models(&self) -> Vec<(u32, Vec<u32>)> {
        Vec::new()
    }
}

/// The effect file of an action (fx/tasks.rs effect_base).
fn sweep_file(category: u8, anim: u32) -> Option<u32> {
    Some(match category {
        3 => 0xd48,
        4 => 0xaf0,
        5 => if anim > 99 { 0xdca5 } else { 0x1330 },
        6 => 0x113c,
        11 => match anim {
            0x800.. => 0x14b07,
            0x600.. => 0xe739,
            0x200.. => 0xc1ef,
            _ => 0xf3c,
        },
        _ => return None,
    } + anim)
}

fn fx_sweep(out: &std::path::Path, kinds: &str, ticks: u32, mob_models: Option<&std::path::Path>, only: &[u32]) -> Result<()> {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;
    let cfg = config::Config::load();
    let index = std::sync::Arc::new(dat::DatIndex::open(&cfg.ffxi_dir())?);
    let keys = dat::zone::Keys::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?)?;
    let models: BTreeMap<u32, String> = mob_models
        .map(std::fs::read_to_string)
        .transpose()?
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut c = l.split('\t');
            Some((c.next()?.parse().ok()?, c.next()?.to_string()))
        })
        .collect();
    let mut fx = fx::Effects::new();
    fx.set_keys(std::sync::Arc::new(keys));
    fx.set_index(index.clone());
    fx.set_global(fx::lib::Library::load(0, index.read(0)?)?);
    fx.load_zone(fx::lib::Library::load(dat::zone::file_id(230), index.read(dat::zone::file_id(230))?)?);
    let frame = fx::Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: 0.5, view: glam::Mat4::IDENTITY, view_rot: glam::Mat4::IDENTITY, eye: glam::Vec3::new(0.0, -2.0, -6.0), draw_distance: 2000.0, area_light: glam::Vec3::ONE, focal: 350.0, moon_phase: 0, weekday: 0 };
    let ranges: Vec<(u8, std::ops::Range<u32>, &str)> = [(3u8, 0..0x100, "ws"), (11, 0..0x1000, "mob"), (4, 0..0x400, "spell"), (6, 0..0x300, "ja"), (5, 0..0x400, "item")]
        .into_iter()
        .filter(|r| kinds.split(',').any(|k| k == r.2))
        .collect();
    let mut rows = String::from("kind\tcategory\tanim\tfile\tcaster\tticks\tleft\tsched_ops\tstream_ops\tno_sched\tmissing\tno_attach\tattach\n");
    // Summary: per gap, the files it touches.
    let mut gaps: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    let mut runs = 0;
    for (category, range, kind) in ranges {
        for anim in range {
            if !only.is_empty() && (only[0] != category as u32 || only.get(1) != Some(&anim)) {
                continue;
            }
            let Some(file) = sweep_file(category, anim) else { continue };
            if index.path(file).is_none() {
                continue;
            }
            let (mob, caster) = match (category, models.get(&anim)) {
                (11, Some(hex)) => {
                    let bytes: Vec<u8> = (0..hex.len() / 2).filter_map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()).collect();
                    match dat::npc::parse_look(&bytes).and_then(|l| dat::npc::load_npc(&index, &l)) {
                        Ok(ch) => ((ch.part_files.clone(), ch.files.clone()), hex.clone()),
                        Err(_) => ((Vec::new(), vec![1300]), "orc".into()),
                    }
                }
                _ => ((Vec::new(), vec![1300]), "orc".into()),
            };
            let actors = SweepActors { mob_files: mob.clone(), fx_files: Default::default() };
            fx.preload(&mob.1);
            fx.reg.audit = Some(fx::Audit::default());
            let (a, t) = if category == 11 { (SWEEP_MOB, SWEEP_PC) } else { (SWEEP_PC, if category == 5 { SWEEP_PC } else { SWEEP_MOB }) };
            let result = net::action::ActionResult { kind: 1, scale: 1, value: 10, message: 1, animation: anim as u16, ..Default::default() };
            let action = net::action::Action { actor: a, category, arg: anim, info: 0, targets: vec![net::action::ActionTarget { id: t, results: vec![result] }] };
            fx.on_action(&action, None);
            let (mut n, mut kicks, mut peak) = (0, 0usize, 0.0f32);
            let started = std::time::Instant::now();
            while n < ticks {
                // Ticks spent while files load would vary with the machine: wait for them (5 s at most a run).
                if fx.loading_count() > 0 && started.elapsed().as_secs() < 5 {
                    fx.poll_loads();
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    continue;
                }
                fx.tick(&frame, &actors);
                sync_fx_files(&mut fx, &actors.fx_files);
                kicks += fx.quakes.len();
                peak = fx.quakes.iter().copied().fold(peak, f32::max);
                n += 1;
                // Started (the file loads in the background) and nothing left to run.
                if n > 30 && fx.task_count() == 0 && fx.pending_count() == 0 {
                    break;
                }
            }
            let left = fx.task_count();
            if !only.is_empty() {
                println!("op 0x5f camera kicks: {kicks}, the largest {peak}");
                let l = (0..3).find_map(|_| {
                    let l = fx.reg.lib_of(file);
                    if l.is_none() {
                        fx.tick(&frame, &actors);
                    }
                    l
                });
                if let Some(l) = l {
                    let lib = &fx.reg.libs[l];
                    for (i, node) in lib.tree.nodes.iter().enumerate().filter(|(_, n)| n.kind == dat::generator::kind::SCHEDULER) {
                        println!("scheduler {} (node {i})", node.name());
                        for op in fx::sched::parse(lib.data(i)) {
                            println!("  {:#04x} wait {:4} dur {:4} {:02x?}", op.code, op.wait, op.dur, &op.raw[..op.raw.len().min(32)]);
                        }
                    }
                }
            }
            fx.reset_runs();
            let au = fx.reg.audit.take().unwrap_or_default();
            let list = |m: &BTreeMap<u8, u32>| m.iter().map(|(k, v)| format!("{k:#04x}x{v}")).collect::<Vec<_>>().join(" ");
            let four = |n: u32| String::from_utf8_lossy(&n.to_le_bytes()).trim_end_matches('\0').to_string();
            let stream: Vec<String> = au.stream.iter().map(|((s, k), v)| format!("{s}/{k:#04x}x{v}")).collect();
            let no_sched: Vec<String> = au.no_sched.keys().map(|n| four(*n)).collect();
            let missing: Vec<String> = au.missing.keys().map(|(f, n, k)| format!("{f}:{}:{k:#x}", four(*n))).collect();
            let attach: Vec<String> = au.attach.iter().map(|((m, a, b), v)| format!("m{m:#x}/{a:#x}/{b:#x}x{v}")).collect();
            let _ = writeln!(rows, "{kind}\t{category}\t{anim}\t{file}\t{caster}\t{n}\t{left}\t{}\t{}\t{}\t{}\t{}\t{}", list(&au.sched), stream.join(" "), no_sched.join(" "), missing.join(" "), list(&au.no_attach), attach.join(" "));
            for k in au.sched.keys() {
                gaps.entry(format!("task op {k:#04x}")).or_default().push(file);
            }
            for (s, k) in au.stream.keys() {
                gaps.entry(format!("stream {s} op {k:#04x}")).or_default().push(file);
            }
            for m in au.no_attach.keys() {
                gaps.entry(format!("attach mode {m:#x} unplaced")).or_default().push(file);
            }
            // What is still not done: point 0x35 (the water height).
            for ((_, a, b), _) in au.attach.iter() {
                if *a == 0x35 || *b == 0x35 {
                    gaps.entry("attach point 0x35 (water height)".into()).or_default().push(file);
                }
            }
            if !no_sched.is_empty() {
                gaps.entry("scheduler not found".into()).or_default().push(file);
            }
            if left > 0 {
                gaps.entry(format!("still running after {ticks} ticks")).or_default().push(file);
            }
            runs += 1;
            if runs % 100 == 0 {
                eprintln!("{runs} files...");
            }
        }
    }
    std::fs::write(out, rows)?;
    let mut ranked: Vec<(String, Vec<u32>)> = gaps.into_iter().collect();
    ranked.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    let mut summary = format!("{runs} files run\n");
    for (gap, files) in &ranked {
        let _ = writeln!(summary, "{:5}  {gap}  e.g. {:?}", files.len(), &files[..files.len().min(8)]);
    }
    std::fs::write(out.with_extension("summary.txt"), &summary)?;
    print!("{summary}");
    Ok(())
}

/// Two stand-in actors for fx-shot: fixed placements facing each other, attach points at chest height.
struct ShotActors {
    at: [(u32, glam::Vec3, glam::Vec3); 2],
    /// The caster's movement a tick, and the ticks walked.
    walk: glam::Vec3,
    ticks: std::cell::Cell<f32>,
    fx_files: std::cell::RefCell<Vec<(u32, u32)>>,
    ground: client::ground::Ground,
}

impl ShotActors {
    fn pos(&self, id: u32) -> Option<glam::Vec3> {
        let a = self.at.iter().find(|a| a.0 == id)?;
        Some(if id == self.at[0].0 { a.1 + self.walk * self.ticks.get() } else { a.1 })
    }
}

impl fx::Actors for ShotActors {
    fn matrix(&self, id: u32) -> Option<glam::Mat4> {
        let (_, _, fwd) = self.at.iter().find(|a| a.0 == id)?;
        let f = fwd.normalize();
        Some(glam::Mat4::from_cols(f.extend(0.0), glam::Vec3::Y.extend(0.0), f.cross(glam::Vec3::Y).extend(0.0), self.pos(id)?.extend(1.0)))
    }
    fn foot(&self, id: u32) -> Option<glam::Vec3> {
        self.pos(id)
    }
    fn point(&self, _: u32, k: u16) -> Option<glam::Vec3> {
        // Feet for point 0, chest otherwise *(stand-in, not a skeleton)*.
        Some(if k == 0 { glam::Vec3::ZERO } else { glam::Vec3::new(0.0, -1.2, 0.0) })
    }
    fn scale(&self, _: u32) -> f32 {
        1.0
    }
    fn files(&self, id: u32) -> (Vec<u32>, Vec<u32>) {
        let (p, mut m) = if id == self.at[0].0 { (vec![8392], vec![9672, 7072]) } else { (Vec::new(), vec![1300]) };
        m.extend(self.fx_files.borrow().iter().filter(|x| x.0 == id).map(|x| x.1));
        (p, m)
    }
    fn hidden(&self, _: u32) -> bool {
        false
    }
    fn models(&self) -> Vec<(u32, Vec<u32>)> {
        Vec::new()
    }
    fn ground_hit(&self, from: glam::Vec3, to: glam::Vec3) -> Option<glam::Vec3> {
        let d = self.ground.raycast(from, to)?;
        Some(from + (to - from).normalize_or_zero() * d)
    }
    fn decal_cell(&self, c: glam::Vec3) -> Option<(usize, usize)> {
        self.ground.decal_cell(c)
    }
    fn decal_tris(&self, c: glam::Vec3) -> Vec<glam::Vec3> {
        self.ground.decal_tris(c)
    }
}

#[allow(clippy::too_many_arguments)]
fn fx_shot(zone: u32, out: &std::path::Path, eye: &[f32], caster: &[f32], victim: &[f32], (category, anim, dust, fx_file): (u8, u16, Option<String>, Option<u32>), walk: &[f32], frames: u32, every: u32, hour: f32) -> Result<()> {
    use glam::Vec3;
    let cfg = config::Config::load();
    let index = std::sync::Arc::new(dat::DatIndex::open(&cfg.ffxi_dir())?);
    let keys = dat::zone::Keys::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?)?;
    let file = index.read(dat::zone::file_id(zone))?;
    let scene = dat::zone::load_scene(&keys, &file)?;
    let lighting = dat::weather::ZoneWeather::parse(&file).ok().and_then(|w| w.at("suny", hour * 60.0)).map(|e| dat::weather::Lighting::from_env(&e, hour * 60.0));
    let v3 = |v: &[f32]| Vec3::new(v[0], v[1], v[2]);
    let (mut c, mut t) = (v3(caster), v3(victim));
    let g = client::ground::Ground::build(&scene);
    // Both on the floor under (or near) the given height.
    c.y = g.snap(c.x, c.z, c.y).unwrap_or(c.y);
    t.y = g.snap(t.x, t.z, t.y).unwrap_or(t.y);
    if dust.is_some() {
        println!("caster on {c} surface {:?}", g.surface(c));
    }
    let walk = if walk.len() == 3 { v3(walk) } else { Vec3::ZERO };
    let actors = ShotActors { at: [(0x0001_0001, c, t - c), (0x0100_0001, t, c - t)], walk, ticks: std::cell::Cell::new(0.0), fx_files: Default::default(), ground: g };
    let mut fx = fx::Effects::new();
    fx.set_keys(std::sync::Arc::new(keys));
    fx.set_index(index.clone());
    fx.set_global(fx::lib::Library::load(0, index.read(0)?)?);
    fx.load_zone(fx::lib::Library::load(dat::zone::file_id(zone), file.clone())?);
    let files = [8392u32, 9672, 7072, 1300];
    fx.preload(&files);
    let result = net::action::ActionResult { kind: 1, scale: 1, value: 10, message: 1, animation: anim, ..Default::default() };
    let action = net::action::Action { actor: 0x0001_0001, category, arg: if category == 1 { u32::from_le_bytes(*b"atk0") } else { anim as u32 }, info: 0, targets: vec![net::action::ActionTarget { id: 0x0100_0001, results: vec![result] }] };
    let camera = render::Camera { eye: v3(eye), target: (c + t) / 2.0 + Vec3::new(0.0, -1.2, 0.0), up: Vec3::NEG_Y, fov_y: 50f32.to_radians() };
    let paths: Vec<PathBuf> = (0..frames).map(|i| out.with_file_name(format!("{}_{i:02}.png", out.file_stem().unwrap().to_string_lossy()))).collect();
    let list: Vec<(&render::Camera, &std::path::Path, Option<dat::weather::Lighting>)> = paths.iter().map(|p| (&camera, p.as_path(), lighting)).collect();
    let mut eg: Option<render::effects::EffectsGpu> = None;
    let mut started = false;
    let mut post = |gpu: &render::Gpu, enc: &mut wgpu::CommandEncoder, (color, color_tex): (&wgpu::TextureView, &wgpu::Texture), depth: &wgpu::TextureView, cam: &render::Camera, l: Option<&dat::weather::Lighting>, zr: &render::ZoneRenderer| {
        let eg = eg.get_or_insert_with(|| render::effects::EffectsGpu::new(gpu, wgpu::TextureFormat::Rgba8Unorm));
        eg.set_lights(&zr.lights);
        fx.reg.light_keys = zr.light_keys.clone();
        let view = cam.view();
        let mut view_rot = view;
        view_rot.w_axis = glam::Vec4::W;
        let f = fx::Frame { dt: 1.0, seconds: 1.0 / 60.0, dayfrac: hour / 24.0, view, view_rot, eye: cam.eye, draw_distance: 2000.0, area_light: l.map_or(Vec3::ONE, |l| l.area), focal: 350.0, moon_phase: 0, weekday: 0 };
        if !started {
            while files.iter().any(|f| fx.reg.lib_of(*f).is_none()) {
                fx.tick(&f, &actors);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            match (&dust, fx_file) {
                (Some(d), _) => {
                    let name = <[u8; 4]>::try_from(d.as_bytes()).ok();
                    fx.footstep(0x0001_0001, c, *b"0000", name.filter(|n| n != b"fmrk"), name == Some(*b"fmrk"), false, 101);
                }
                (None, Some(n)) => {
                    let who = fx::sched::Who::Actor(0x0001_0001);
                    fx.on_event_scheduler(n, u32::from_le_bytes(*b"main"), who, who);
                }
                (None, None) => fx.on_action(&action, (category == 1).then(|| "ati0".to_string())),
            }
            started = true;
        }
        for _ in 0..every {
            fx.tick(&f, &actors);
            sync_fx_files(&mut fx, &actors.fx_files);
            actors.ticks.set(actors.ticks.get() + 1.0);
            let mut e = gpu.device.create_command_encoder(&Default::default());
            eg.update(gpu, &mut e, &mut fx, &f);
            gpu.queue.submit([e.finish()]);
            let _ = gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
            eg.after_submit(gpu, &mut fx);
        }
        let live: Vec<String> = fx.describe().into_iter().filter(|d| !d.contains(" t_") || d.contains("/fses/")).collect();
        println!("{} live, {} tasks: {live:?}", fx.live_count(), fx.task_count());
        let (fog_color, fog) = l.map_or(([0.0; 3], [150.0, 600.0]), |l| (l.fog.to_array(), [l.fog_start, l.fog_end]));
        eg.draw(gpu, enc, (color, depth, [1600, 900]), Some(color_tex), &fx, (cam.proj(1600.0 / 900.0), cam.view(), cam.eye), [render::NEAR, render::FAR], fog_color, fog);
    };
    render::render_pngs_post(&scene, &list, 1600, 900, &|_, _| Vec::new(), &mut post)?;
    println!("wrote {} frames", paths.len());
    Ok(())
}
