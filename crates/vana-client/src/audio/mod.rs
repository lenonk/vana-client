//! Music and sound effects as the client plays them: file formats, the sound thread (mixer), and the
//! game-side music logic (CYyMusicLoadTask FUN_100365e0, zone music FUN_100926a0, title FUN_1023ae50).

pub mod atrac3;
pub mod format;
pub mod mixer;
pub mod output;

use mixer::{Cmd, Mixer, Status};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

/// Menu sounds: type-0x3d "SeSep" resources in DAT 0 named "0001".. (FUN_10036d10 ..), = se ids.
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub enum Ui {
    /// "0001": cursor moved (menu events 1/2).
    Cursor = 1,
    /// "0002": item chosen (event 5).
    Select = 2,
    /// "0003": cancel / back (event 6).
    Cancel = 3,
    /// "0004": not allowed.
    Error = 4,
    /// "0013": a menu closed (with FUN_10157290).
    Close = 13,
    /// "0014": main menu opened (FUN_1013b080).
    Open = 14,
}

/// Music after an expansion's title track: {track, ticks before the switch, next} (FFXiMain 0x10351718).
const SEQUENCE: [(u32, i32, u32); 7] = [(0, 0, 0), (108, 0, 108), (242, 8100, 108), (179, 7140, 108), (140, 8160, 108), (58, 8160, 108), (34, 7740, 108)];
/// Title track choice n -> (installed-mask bit, sequence entry) (FUN_102455a0).
const TITLE_PICK: [(u32, usize); 6] = [(0, 1), (2, 2), (3, 3), (4, 4), (11, 5), (0, 6)];
/// Title music option 0xaf -> (mode, n) (FFXiMain 0x1033cc48): mode 0 newest installed, 1 track n, 2 random.
const TITLE_MODE: [(u8, u32); 8] = [(0, 0), (2, 0), (1, 0), (1, 1), (1, 2), (1, 3), (1, 4), (1, 5)];
pub const TITLE_DEFAULT: u32 = 0x6c;

/// The music change task: fade out, stop, start, optionally switch to a follow-up track.
struct Task {
    state: u8,
    /// 0x34: save the outgoing track's position first.
    save: bool,
    /// 0x35: only fade out; the caller starts the music itself.
    no_start: bool,
    id: u32,
    vol: i32,
    vol2: i32,
    fade_in: i32,
    counter: i32,
    seq: usize,
    /// 0x15: no follow-up track.
    single: bool,
    wait: i32,
}

pub struct Audio {
    /// Requests to the loader thread, which reads files and feeds the mixer in order.
    tx: Sender<Req>,
    pub status: Arc<Status>,
    _out: output::Output,
    /// Track wanted (DAT_10451008) and started (DAT_10451004).
    wanted: u32,
    playing: u32,
    task: Option<Task>,
    /// Music volume multipliers (DAT_10346650 x DAT_10346654) and the effects level factor (DAT_10346568).
    music: f32,
    effects: f32,
    tick_acc: f32,
    /// Event sound volumes (ops 0x69 / 0x6a): music (DAT_10346570) and effects (DAT_1034656c).
    event_se: f32,
    event_stream: f32,
    event_ui: f32,
    /// Voices started as the scheduler's stream.
    streams: std::collections::HashSet<u32>,
    /// Level of the track playing, for re-applying the event volume.
    cur_vol: i32,
    /// Config-tool 0029: effects at once; a new one is dropped while that many play (FUN_10037510).
    pub max_se: u32,
}

impl Audio {
    /// None when there is no output device; the game runs silent.
    pub fn open(ffxi_dir: PathBuf, device: Option<&str>, music: f32, effects: f32) -> Option<Self> {
        let status = Arc::new(Status::default());
        let (mixer_tx, rx) = channel();
        let st = status.clone();
        match output::open(device, move |rate| Mixer::new(rx, rate, st)) {
            Ok(out) => {
                log::info!("audio: {} at {} Hz", out.host, out.rate);
                let (tx, req) = channel();
                std::thread::Builder::new().name("audio-load".into()).spawn(move || loader(ffxi_dir, req, mixer_tx)).ok()?;
                Some(Self { tx, status, _out: out, wanted: 0, playing: 0, task: None, music, effects, tick_acc: 0.0, event_se: 1.0, event_stream: 1.0, event_ui: 1.0, streams: Default::default(), cur_vol: 0x7f, max_se: 12 })
            }
            Err(e) => {
                log::warn!("audio disabled: {e:#}");
                None
            }
        }
    }

    fn send(&self, c: Cmd) {
        let _ = self.tx.send(Req::Cmd(c));
    }

    /// Volume level for a track volume (0..127) under the config multipliers.
    fn level(&self, vol: i32) -> u8 {
        ((vol as f32 * self.music) as i32).clamp(0, 127) as u8
    }

    fn start(&mut self, id: u32, vol: i32) {
        self.cur_vol = vol;
        log::info!("music: start {id} at level {}", self.level(vol));
        let _ = self.tx.send(Req::Music { id, level: self.level(vol) });
    }

    /// FUN_10036530: change music through the fade task (ignored when `id` is already wanted).
    pub fn change_music(&mut self, id: u32, vol: i32, vol2: i32, fade_in: i32, fade_out: i32, save: bool) {
        self.change(id, vol, vol2, fade_in, fade_out, save, false, 0, false);
    }

    #[allow(clippy::too_many_arguments)]
    fn change(&mut self, id: u32, vol: i32, vol2: i32, fade_in: i32, fade_out: i32, save: bool, no_start: bool, seq: usize, single: bool) {
        if self.wanted == id {
            return;
        }
        self.wanted = id;
        self.task = Some(Task { state: 0, save, no_start, id, vol, vol2, fade_in, counter: fade_out.max(0x1e), seq, single, wait: 0 });
    }

    /// FUN_10036b40: fade the music out for a track the caller will start (creator race music).
    pub fn prepare_music(&mut self, id: u32) {
        self.change(id, 0x7f, 0x7f, 0, 0x1e, false, true, 0, false);
    }

    /// FUN_10036a90: start now (stopping a different track at once).
    pub fn play_music_now(&mut self, id: u32) {
        self.task = None;
        if self.wanted != id {
            self.send(Cmd::BgmFadeOut { ticks: 0 });
            self.send(Cmd::BgmStop);
        }
        self.wanted = id;
        self.start(id, 0x7f);
        self.playing = id;
    }

    /// FUN_1023ae50 / FUN_10036980: title music for option 0xaf and the installed-content mask.
    pub fn title_music(&mut self, option: u32, installed: u32) {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u32).unwrap_or(0);
        let (id, seq, single) = title_choice(option, installed, t);
        self.change(id, 0x7f, 0x7f, 0, 0, false, false, seq, single);
    }

    /// New config factors (0..1): the playing track takes the new level over 10 ticks.
    pub fn set_volumes(&mut self, music: f32, effects: f32) {
        if (music - self.music).abs() > 1e-4 {
            self.music = music;
            let level = self.level(self.cur_vol);
            self.send(Cmd::BgmVolume { level, ticks: 10 });
        }
        self.effects = effects;
    }

    pub fn stop_music(&mut self, ticks: i32) {
        self.task = None;
        self.wanted = 0;
        self.send(Cmd::BgmFadeOut { ticks });
    }

    /// Unpositioned world effect at a level 0..127 before the config factor.
    pub fn play_se(&mut self, id: u32, level: i32) {
        self.se_at(id, level, self.event_se);
    }

    /// An interface sound (menus, targeting): the event volume's mask 2 / 8 level (DAT_10346568).
    pub fn play_ui_se(&mut self, id: u32) {
        self.se_at(id, 127, self.event_ui);
    }

    fn se_at(&mut self, id: u32, level: i32, event: f32) {
        if self.status.voices.load(std::sync::atomic::Ordering::Relaxed) >= self.max_se {
            return;
        }
        let lv = ((level as f32 * self.effects * event) as i32).clamp(0, 127) as u8;
        let _ = self.tx.send(Req::Se { id, level: lv });
    }

    /// A positioned effect voice (FUN_10036130 -> cmd 2) under `handle`; level 0..127 before the effects factor,
    /// pan 0..127. Refused (false) while the busy voices reach the limit (DAT_1044417c).
    pub fn play_voice(&mut self, handle: u32, id: u32, level: i32, pan: u8, stream: bool) -> bool {
        if self.status.voices.load(std::sync::atomic::Ordering::Relaxed) >= self.max_se {
            return false;
        }
        if stream {
            self.streams.insert(handle);
        }
        let lv = self.voice_level_of(handle, level);
        let _ = self.tx.send(Req::Voice { id, level: lv, pan: pan & 0x7f, handle });
        true
    }

    /// A voice's level: world effects take the event's effect volume, the stream only mask 8.
    fn voice_level_of(&self, handle: u32, level: i32) -> u8 {
        if self.streams.contains(&handle) { ((level as f32 * self.effects * self.event_stream) as i32).clamp(0, 127) as u8 } else { self.se_level(level) }
    }

    pub fn voice_level(&mut self, handle: u32, level: i32) {
        let lv = self.voice_level_of(handle, level);
        self.send(Cmd::SeLevel { handle, level: lv });
    }

    pub fn voice_pan(&mut self, handle: u32, pan: u8) {
        self.send(Cmd::SePan { handle, pan: pan & 0x7f });
    }

    /// Stop now (0) or fade out over 60 Hz ticks.
    pub fn voice_stop(&mut self, handle: u32, ticks: i32) {
        self.send(Cmd::SeStop { handle, ticks });
    }

    /// Handles of the voices still playing.
    pub fn alive(&self) -> Vec<u32> {
        self.status.alive.lock().map(|a| a.clone()).unwrap_or_default()
    }

    fn se_level(&self, level: i32) -> u8 {
        ((level as f32 * self.effects * self.event_se) as i32).clamp(0, 127) as u8
    }

    /// Event volume (0x69 set / 0x6a fade, FUN_100b5c30 / FUN_10034be0): mask 1 world effects (DAT_1034656c), 2 interface
    /// (DAT_10346568), 4 the zone's ambient loop (DAT_10346570, the voice FUN_10035350 starts; we play none), 8 all of
    /// them and the stream. The music is not among them: cutscenes mute the world and keep the music.
    pub fn event_volume(&mut self, mask: i32, level: f32, _ticks: f32) {
        let level = level.clamp(0.0, 1.0);
        // FUN_100b5c30: 1 world effects (FUN_10035200), 8 everything and the stream (FUN_100350e0).
        if mask & 0x9 != 0 {
            self.event_se = level;
        }
        if mask & 0x8 != 0 {
            self.event_stream = level;
        }
        // 2 interface sounds (FUN_10035250).
        if mask & 0xa != 0 {
            self.event_ui = level;
        }
    }

    /// Event ops 0x5d and 0x5c 0xa0 / 0xa1 (FUN_10037a30): the music fades to `vol` over `ticks`.
    pub fn music_fade(&mut self, vol: i32, ticks: i32) {
        self.cur_vol = vol;
        let level = self.level(vol);
        self.send(Cmd::BgmVolume { level, ticks: ticks.max(0) });
    }

    /// A music change is still under way (event op 0x9a waits for it).
    pub fn music_changing(&self) -> bool {
        self.task.is_some()
    }

    pub fn ui(&mut self, s: Ui) {
        self.play_ui_se(s as u32);
    }

    /// Advances the music task by real time at the client's 60 Hz tick.
    pub fn update(&mut self, dt: f32) {
        self.tick_acc += dt * 60.0;
        let ticks = self.tick_acc.floor().min(30.0) as i32;
        self.tick_acc -= ticks as f32;
        if self.tick_acc > 1.0 {
            self.tick_acc = 0.0;
        }
        for _ in 0..ticks {
            self.step();
        }
    }

    /// One tick of FUN_100365e0.
    fn step(&mut self) {
        let Some(mut t) = self.task.take() else { return };
        let mut done = false;
        match t.state {
            0 => {
                if t.save {
                    self.send(Cmd::SavePosition);
                }
                self.send(Cmd::BgmFadeOut { ticks: t.counter });
                t.state = 1;
            }
            1 => {
                t.counter -= 1;
                if t.counter < 0 {
                    self.send(Cmd::BgmFadeOut { ticks: 0 });
                    t.counter = 4;
                    t.state = 2;
                }
            }
            2 => {
                t.counter -= 1;
                if t.counter < 0 {
                    self.send(Cmd::BgmStop);
                    t.state = 3;
                }
            }
            3 => {
                self.playing = t.id;
                if !t.no_start {
                    self.start(t.id, t.vol);
                }
                t.wait = if t.vol != t.vol2 { 10 } else { 0 };
                t.state = 4;
            }
            4 => {
                if t.wait != 0 {
                    t.wait -= 1;
                } else {
                    if !t.no_start {
                        self.send(Cmd::BgmVolume { level: self.level(t.vol2), ticks: t.fade_in });
                    }
                    let (_, wait, _) = SEQUENCE[t.seq];
                    if wait != 0 && !t.single {
                        t.counter = wait;
                        t.state = 5;
                    } else {
                        done = true;
                    }
                }
            }
            5 => {
                t.counter -= 1;
                if t.counter < 0 {
                    t.counter = 0x168;
                    self.send(Cmd::BgmFadeOut { ticks: 0x168 });
                    t.state = 6;
                }
            }
            6 => {
                t.counter -= 1;
                if t.counter < 0 {
                    self.send(Cmd::BgmFadeOut { ticks: 0 });
                    t.counter = 4;
                    t.state = 7;
                }
            }
            7 => {
                t.counter -= 1;
                if t.counter < 0 {
                    t.id = SEQUENCE[t.seq].2;
                    self.wanted = t.id;
                    self.send(Cmd::BgmStop);
                    t.state = 8;
                }
            }
            _ => {
                self.start(t.id, t.vol2);
                self.playing = t.id;
                done = true;
            }
        }
        if !done {
            self.task = Some(t);
        }
    }

    /// Cmd 11: forget saved music positions.
    pub fn clear_positions(&mut self) {
        self.send(Cmd::ClearPositions(None));
    }
}

enum Req {
    Cmd(Cmd),
    Music { id: u32, level: u8 },
    Se { id: u32, level: u8 },
    Voice { id: u32, level: u8, pan: u8, handle: u32 },
}

/// FUN_100377a0's choice: (track, sequence entry, no follow-up) for option 0xaf;  stands for timeGetTime.
pub fn title_choice(option: u32, installed: u32, t: u32) -> (u32, usize, bool) {
    let (mode, n) = TITLE_MODE.get(option as usize).copied().unwrap_or((0, 0));
    let (mut n, single) = match mode {
        0 => (TITLE_PICK.len() as i32 - 1, false),
        1 => (n as i32, true),
        2 => ((t % TITLE_PICK.len() as u32) as i32, true),
        _ => (0, false),
    };
    // An expansion not installed falls back to the next older one.
    let mut seq = 0;
    while n >= 0 {
        let (bit, entry) = TITLE_PICK[n as usize];
        if installed & (1 << bit) != 0 {
            seq = entry;
            break;
        }
        n -= 1;
    }
    let id = if SEQUENCE[seq].0 == 0 { TITLE_DEFAULT } else { SEQUENCE[seq].0 };
    (id, seq, single)
}

/// Reads sound files off the UI thread, keeping command order (effects stay cached).
fn loader(dir: PathBuf, rx: Receiver<Req>, tx: Sender<Cmd>) {
    let mut effects: HashMap<u32, Option<Arc<[u8]>>> = HashMap::new();
    let read = |path: Option<PathBuf>, what: &str, id: u32| -> Option<Arc<[u8]>> {
        let bytes = path.and_then(|p| std::fs::read(p).ok()).map(Arc::from);
        if bytes.is_none() {
            log::warn!("no {what} {id}");
        }
        bytes
    };
    for r in rx {
        let cmd = match r {
            Req::Cmd(c) => Some(c),
            Req::Music { id, level } => read(format::music_path(&dir, id), "music", id).map(|bytes| Cmd::PlayBgm { id, bytes, level, ticks: 0 }),
            Req::Se { id, level } => {
                log::debug!("se {id} at level {level}");
                effects.entry(id).or_insert_with(|| read(format::se_path(&dir, id), "sound", id)).clone().map(|bytes| Cmd::PlaySe { bytes, level, pan: 0x40, priority: 0, handle: 0 })
            }
            Req::Voice { id, level, pan, handle } => effects.entry(id).or_insert_with(|| read(format::se_path(&dir, id), "sound", id)).clone().map(|bytes| Cmd::PlaySe { bytes, level, pan, priority: 0, handle }),
        };
        if let Some(c) = cmd
            && tx.send(c).is_err()
        {
            return;
        }
    }
}

/// What the player is doing, for the zone music choice.
#[derive(Clone, Copy, Default)]
pub struct MusicContext {
    /// Own status (POS_HEAD 0x1F / 0x037).
    pub status: u8,
    /// Vana'diel hour 0..23.
    pub hour: u32,
    /// Party members including us.
    pub party: u32,
}

/// The zone's music slots (0x00A MusicNum[5] -> 0..4, death 0x6f -> 5, 0x05F any of 0..7) and the
/// choice among them (FUN_100926a0).
pub struct ZoneMusic {
    pub slots: [u16; 8],
    pub vols: [u16; 8],
    last_id: u16,
    last_slot: u8,
    field_slot: u8,
    /// Slots arrive with the first 0x00A; nothing is chosen before.
    ready: bool,
}

impl Default for ZoneMusic {
    fn default() -> Self {
        Self { slots: [0; 8], vols: [0x7f; 8], last_id: 0xffff, last_slot: 0xff, field_slot: 0, ready: false }
    }
}

/// A music change the selector asks for.
#[derive(Debug, PartialEq, Eq)]
pub struct Change {
    pub id: u32,
    pub vol: i32,
    pub fade_in: i32,
    pub save: bool,
    pub clear: bool,
}

impl ZoneMusic {
    /// 0x00A: slots 0..4 from MusicNum, slot 5 the death track 0x6f.
    pub fn zone_in(&mut self, music: [u16; 5]) {
        self.slots[..5].copy_from_slice(&music);
        self.slots[5] = 0x6f;
        self.ready = true;
    }

    /// Event op 0x5c (FUN_100b6e30): slot `slot` plays `id`, started at `vol` (0x7f, or the 0x80..0x87 forms' operand,
    /// at most 0xff); any 0x5c makes the selector choose again.
    pub fn set_slot_vol(&mut self, slot: u16, id: u16, vol: u16) {
        if slot < 8 {
            self.vols[slot as usize] = vol.min(0xff);
        }
        self.set_slot(slot, id);
    }

    /// 0x05F {u16 slot @4, u16 track @6} (handler 0x10098150).
    pub fn set_slot(&mut self, slot: u16, id: u16) {
        if slot < 8 {
            self.slots[slot as usize] = id;
            self.last_id = 0xffff;
        }
    }

    pub fn select(&mut self, cx: MusicContext) -> Option<Change> {
        if !self.ready {
            return None;
        }
        let slot = match cx.status {
            2 | 3 => 5,
            5 | 0x55 if self.slots[4] != 0 => 4,
            // Fishing fights use slot 7 when the zone has one (the 0x10c sub-state test is not modelled).
            0x26 | 0x39 => {
                if self.slots[7] == 0 {
                    return None;
                }
                7
            }
            1 => {
                if cx.party <= 1 {
                    2
                } else {
                    3
                }
            }
            _ if cx.hour < 6 || cx.hour > 17 => 1,
            _ => 0,
        };
        let id = self.slots[slot];
        if self.last_id == id {
            return None;
        }
        let save = self.last_slot <= 1;
        if save {
            self.field_slot = self.last_slot;
        }
        let back = self.field_slot as usize == slot && matches!(self.last_slot, 2 | 3 | 7);
        self.last_id = id;
        self.last_slot = slot as u8;
        Some(Change { id: id as u32, vol: self.vols[slot] as i32, fade_in: if back { 0x3c } else { 0 }, save, clear: !back })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_choice_follows_ffximain() {
        // ROM2..ROM9 (this install) -> 0xfff; without ROM9 -> 0x7ff.
        let all = crate::dat::installed_mask(|n| n <= 9);
        assert_eq!(all, 0xfff);
        assert_eq!(crate::dat::installed_mask(|n| n <= 8), 0x7ff);
        assert_eq!(crate::dat::installed_mask(|n| n == 6 || n == 9), 0x801);
        let pick = |o, m| title_choice(o, m, 0);
        // Newest: track 34 (base game, entry 6) then 108; fixed 1..6 = 108, 242, 179, 140, 58, 34.
        assert_eq!(pick(0, all), (34, 6, false));
        assert_eq!([2, 3, 4, 5, 6, 7].map(|o| pick(o, all).0), [108, 242, 179, 140, 58, 34]);
        assert!([2, 3, 4, 5, 6, 7].iter().all(|&o| pick(o, all).2));
        // Random: timeGetTime % 6, falling back below a missing one.
        assert_eq!((0..6).map(|t| title_choice(1, all, t).0).collect::<Vec<_>>(), [108, 242, 179, 140, 58, 34]);
        assert_eq!(title_choice(1, 0x7ff, 4).0, 140);
        // Without ROM9 Seekers' 58 falls back to 140; newest still needs only the base game.
        assert_eq!(pick(0, 0x7ff), (34, 6, false));
        assert_eq!(pick(6, 0x7ff).0, 140);
        assert_eq!(pick(0, 1), (34, 6, false));
    }

    #[test]
    fn battle_and_back_resumes_the_field_track() {
        let mut z = ZoneMusic::default();
        z.zone_in([101, 102, 103, 104, 0]);
        let day = MusicContext { status: 0, hour: 12, party: 1 };
        assert_eq!(z.select(day), Some(Change { id: 101, vol: 0x7f, fade_in: 0, save: false, clear: true }));
        assert_eq!(z.select(day), None);
        let fight = MusicContext { status: 1, ..day };
        assert_eq!(z.select(fight), Some(Change { id: 103, vol: 0x7f, fade_in: 0, save: true, clear: true }));
        assert_eq!(z.select(day), Some(Change { id: 101, vol: 0x7f, fade_in: 0x3c, save: false, clear: false }));
        let night = MusicContext { hour: 20, ..day };
        assert_eq!(z.select(night).map(|c| c.id), Some(102));
        assert_eq!(z.select(MusicContext { status: 3, ..day }).map(|c| c.id), Some(0x6f));
    }
}
