//! Live character creator preview: the dancer performance (creatorshot's traced shading) drawn over
//! zone 744 (133 in 2019) lit at 11:00 ('suny', FUN_1022ef00). View 0/1 follow the recorded performance cameras,
//! view 2 is the idle loop with an orbit camera. The zone is drawn from the dancer's camera (FUN_102451f0).

use crate::dat::dancer::{self, Channels, Model, SkinPrep, creator::{BODY_SCALE, LIGHTS, MODEL_GROUP, MOTION_GROUP}};
use crate::dat::weather::{Lighting, ZoneWeather};
use crate::dat::{DatIndex, zone};
use crate::render::dancer::{DancerRenderer, Light};
use crate::render::{Camera, Gpu, ZoneRenderer};
use anyhow::{Context, Result};
use glam::{Mat3, Mat4, Quat, Vec3};
use std::sync::Arc;
use std::time::Instant;

/// Lobby clock for the creator (FUN_1022ef00: FUN_10184f30(0xb)).
const MINUTE: f32 = 11.0 * 60.0;
/// Dancer units per zone unit: FUN_102451f0 scales the camera translation by 0.1 (0x1037ca2c).
const ZONE_SCALE: f32 = 0.1;

pub struct Backdrop {
    scene: zone::ZoneScene,
    lighting: Option<Lighting>,
    fx: Option<super::select::FxLibs>,
    /// Shadow colour c = background sun colour / 4 (light 4, FUN_10245bc0 / FUN_1017ff50).
    shadow_c: Vec3,
}

impl Backdrop {
    pub fn load(index: &Arc<DatIndex>, keys: &Arc<zone::Keys>) -> Result<Self> {
        let file = index.read(zone::file_id(super::select::ZONE))?;
        let scene = zone::load_scene(keys, &file)?;
        let fx = super::select::FxLibs::load(index, keys, &file);
        let env = ZoneWeather::parse(&file).ok().and_then(|w| w.at("suny", MINUTE));
        let lighting = env.as_ref().map(|e| Lighting::from_env(e, MINUTE));
        let shadow_c = env.map_or(Vec3::splat(0.2), |e| Vec3::new(e.bg.sun[0] as f32, e.bg.sun[1] as f32, e.bg.sun[2] as f32) / 255.0 / 4.0);
        Ok(Self { scene, lighting, fx, shadow_c })
    }
}

/// One race/face's dancer models, performance and idle channels and the two recorded cameras.
pub struct Dancer {
    pub race: usize,
    pub face: usize,
    body: Model,
    head: Model,
    body_prep: SkinPrep,
    head_prep: SkinPrep,
    perf: (Channels, Channels),
    idle: (Channels, Channels),
    cams: [(Channels, Channels); 2],
    /// Performance foley: (body frame, se id), track 0 of 31728 + race, type-0 events.
    foley: Vec<(f32, u32)>,
}

/// Creator foley: "OC:01.00" trigger tracks {type, frame, value}; type 0 plays the type-0x3d "SeSep"
/// resource named "%04d" (value) from 31736 + race, whose u32 @8 is the se id (sqopSystemBuild FUN_1027f210).
fn load_foley(index: &DatIndex, race: usize) -> Result<Vec<(f32, u32)>> {
    let trig = index.read(31728 + race as u32)?;
    let sounds = index.read(31736 + race as u32)?;
    let ids: std::collections::HashMap<String, u32> = crate::dat::chunks(&sounds)?
        .iter()
        .filter(|c| c.kind == 0x3d && c.data.len() >= 12 && c.data.starts_with(b"SeSep"))
        .map(|c| (c.name(), u32::from_le_bytes(c.data[8..12].try_into().unwrap())))
        .collect();
    let u = |o: usize| trig.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    anyhow::ensure!(trig.starts_with(b"OC:01.00"), "not a trigger file");
    let tracks = u(16).unwrap_or(0);
    let mut out = Vec::new();
    let mut at = 24;
    for t in 0..tracks {
        let n = u(at + 4).context("short trigger file")? as usize;
        at += 12;
        for e in 0..n {
            let (kind, frame, value) = (u(at + e * 12).unwrap_or(1), u(at + e * 12 + 4).unwrap_or(0), u(at + e * 12 + 8).unwrap_or(0));
            if t == 0
                && kind == 0
                && let Some(&id) = ids.get(&format!("{value:04}"))
            {
                out.push((frame as f32, id));
            }
        }
        at += n * 12;
    }
    Ok(out)
}

impl Dancer {
    pub fn load(index: &DatIndex, race: usize, face: usize) -> Result<Self> {
        let (mg, mo) = (MODEL_GROUP[race], MOTION_GROUP[race]);
        let load = |id: u32, tag: &str| -> Result<Model> { dancer::load_model(&index.read(id)?, &index.read(id + 1)?, tag).with_context(|| format!("dancer model {id}")) };
        let chan = |id: u32| -> Result<Channels> { dancer::parse_channels(&index.read(id)?).with_context(|| format!("channels {id}")) };
        let f = face as u32;
        let (body, head) = (load(mg + 2, "b:")?, load(mg + 4 + 2 * f, "h:")?);
        Ok(Self {
            race,
            face,
            body_prep: dancer::skin_prep(&body),
            head_prep: dancer::skin_prep(&head),
            body,
            head,
            perf: (chan(mo + 8)?, chan(mo + 0xe + 6 * f)?),
            idle: (chan(mo + 9)?, chan(mo + 0xf + 6 * f)?),
            cams: [(chan(mo)?, chan(mo + 1)?), (chan(mo + 2)?, chan(mo + 3)?)],
            foley: load_foley(index, race).unwrap_or_else(|e| {
                log::warn!("creator foley {race}: {e:#}");
                Vec::new()
            }),
        })
    }
}

/// The game camera FUN_102451f0 derives from the dancer camera each frame: rotation rows 1 and 2 and the
/// translation negated, translation x 0.1, i.e. dancer (x, y, z) = 10 x zone (x, -y, -z).
pub fn zone_camera(cam: Mat4, fov_y: f32) -> Camera {
    let f = Vec3::new(1.0, -1.0, -1.0);
    let eye = cam.w_axis.truncate() * f * ZONE_SCALE;
    let fwd = -cam.z_axis.truncate() * f;
    Camera { eye, target: eye + fwd, up: cam.y_axis.truncate() * f, fov_y }
}

/// The GPU dancer for a race, face and hair; the 30 Hz frame its "_sort" orders and idle bounds were taken at.
struct Posed {
    key: (usize, usize, usize),
    gpu: crate::render::dancer::DancerGpu,
    sorted_at: Option<(i64, u8)>,
    bounds: (Vec3, Vec3),
}

pub struct Creator {
    backdrop: Backdrop,
    zone: Option<ZoneRenderer>,
    pub effects: Option<super::select::LobbyFx>,
    renderer: Option<DancerRenderer>,
    dancer: Option<Dancer>,
    /// Hair shown (0 = A, 1 = B) and the hair the head's materials are switched to.
    pub hair: usize,
    applied_hair: Option<usize>,
    textures_for: Option<(usize, usize)>,
    /// Performance frame (30 per second), view mode and orbit angle for view 2.
    frame: f32,
    pub view: u8,
    pub orbit: f32,
    /// The performance (re)started this frame (the race music starts then, FUN_10245920).
    pub restarted: Option<usize>,
    /// Foley se ids due this frame.
    pub sounds: Vec<u32>,
    posed: Option<Posed>,
    /// Time spent posing and building passes, for the creator-shot bench.
    pub pose_time: std::time::Duration,
    /// Off on the nation step: the dancer is torn down (FUN_10246f10(.., 0)) and only the zone stays.
    pub show_dancer: bool,
}

impl Creator {
    pub fn new(mut backdrop: Backdrop) -> Self {
        Self {
            effects: backdrop.fx.take().map(|l| super::select::LobbyFx::new(l, MINUTE)),
            backdrop,
            zone: None,
            renderer: None,
            dancer: None,
            hair: 0,
            applied_hair: None,
            textures_for: None,
            frame: 0.0,
            view: 0,
            orbit: 0.0,
            restarted: None,
            sounds: Vec::new(),
            posed: None,
            pose_time: Default::default(),
            show_dancer: true,
        }
    }

    pub fn wants(&self) -> Option<(usize, usize)> {
        self.dancer.as_ref().map(|d| (d.race, d.face))
    }

    /// A new race restarts the performance and its music (FUN_10242a70); a new face of the same race
    /// only swaps the face and the dance carries on (FUN_10242eb0).
    pub fn set_dancer(&mut self, d: Dancer) {
        if self.dancer.as_ref().is_none_or(|o| o.race != d.race) {
            self.restarted = Some(d.race);
            self.frame = 0.0;
        }
        self.dancer = Some(d);
        self.applied_hair = None;
        self.posed = None;
    }

    /// Views cycle 0 -> 1 -> 2 -> 0 (FUN_10243450).
    pub fn next_view(&mut self) {
        self.view = (self.view + 1) % 3;
        self.frame = 0.0;
    }

    /// Jumps to a performance frame (creator-shot).
    pub fn seek(&mut self, frame: f32) {
        self.frame = frame;
    }

    pub fn advance(&mut self, dt: f32) {
        let before = self.frame;
        self.frame += dt * 30.0;
        if let Some(fx) = &mut self.effects {
            fx.advance(dt);
        }
        self.orbit += if self.view == 2 { dt * 0.25 } else { 0.0 };
        // The performance loops when it ends (FUN_10245920); torn down on the nation step, it makes no sound.
        if let Some(d) = self.dancer.as_ref().filter(|_| self.show_dancer) {
            if self.view != 2 {
                let first = before == 0.0;
                self.sounds.extend(d.foley.iter().filter(|(f, _)| (*f > before || (first && *f == 0.0)) && *f <= self.frame).map(|&(_, id)| id));
            }
            let len = if self.view == 2 { d.idle.0.frames } else { d.perf.0.frames } as f32;
            if len > 1.0 && self.frame >= len - 1.0 {
                self.frame = 0.0;
                if self.view != 2 {
                    self.restarted = Some(d.race);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(&mut self, gpu: &Gpu, format: wgpu::TextureFormat, encoder: &mut wgpu::CommandEncoder, target: super::select::Target, depth: &wgpu::TextureView, aspect: f32, alive: Option<Vec<u32>>) {
        let view = target.0;
        let zone = self.zone.get_or_insert_with(|| ZoneRenderer::new(gpu, &self.backdrop.scene, format));
        zone.lighting = self.backdrop.lighting;
        let r = self.renderer.get_or_insert_with(|| DancerRenderer::new(gpu, format));
        let Some(d) = &mut self.dancer else {
            // Before the first dancer loads: the opening camera of FUN_1022ef00, eye (0, -5, 40) on the origin.
            let camera = Camera { eye: Vec3::new(0.0, -5.0, 40.0), target: Vec3::ZERO, up: Vec3::NEG_Y, fov_y: crate::dat::title::fov_y(350.0) };
            zone.render(gpu, encoder, view, depth, &camera, aspect, &[]);
            if let Some(fx) = &mut self.effects {
                fx.draw(gpu, format, encoder, target, depth, zone, &camera, aspect, alive);
            }
            return;
        };
        if self.applied_hair != Some(self.hair) {
            crate::creatorshot::apply_hair(&mut d.head, self.hair);
            self.applied_hair = Some(self.hair);
            self.posed = None;
        }
        if self.textures_for != Some((d.race, d.face)) {
            let textures: Vec<_> = d.body.textures.iter().chain(&d.head.textures).cloned().collect();
            r.load_textures(gpu, &textures);
            self.textures_for = Some((d.race, d.face));
        }
        // Every frame, at the fractional performance frame (keys interpolated: lerp, quaternion slerp): only the
        // joint matrices and pass uniforms go up; skinning and the per-vertex shading run in vs_skin.
        let t0 = std::time::Instant::now();
        let key = (d.race, d.face, self.hair);
        if self.posed.as_ref().is_none_or(|p| p.key != key) {
            let gpu_d = crate::render::dancer::DancerGpu::new(gpu, [&d.body, &d.head], [&d.body_prep, &d.head_prep]);
            self.posed = Some(Posed { key, gpu: gpu_d, sorted_at: None, bounds: (Vec3::splat(-1.0), Vec3::splat(1.0)) });
        }
        let idle = self.view == 2;
        let (bc, hc) = if idle { (&d.idle.0, &d.idle.1) } else { (&d.perf.0, &d.perf.1) };
        let fr = |c: &Channels| self.frame.min(c.frames as f32 - 1.0).max(0.0);
        let (bf, hf) = (bc.sample(fr(bc)), hc.sample(fr(hc)));
        let (bm, hm) = (dancer::joint_mats(&d.body, &d.body_prep, Some(&bf)), dancer::joint_mats(&d.head, &d.head_prep, Some(&hf)));
        // Body scale by race and face about the body root (FUN_102460b0 / FUN_10245500).
        let s = BODY_SCALE[d.race.min(7)][d.face.min(7)];
        let root = dancer::pose(&d.body.bones, Some(&bf)).first().map_or(Vec3::ZERO, |m| m.w_axis.truncate());
        let posed = self.posed.as_mut().unwrap();
        // "_sort" orders (and the idle camera's bounds) from CPU positions, once per 30 Hz frame.
        let tick = (self.frame.floor() as i64, self.view);
        let mut sorted = Vec::new();
        let refresh = posed.sorted_at != Some(tick);
        let scaled = |m: usize, p: Vec3| if m == 0 { root + (p - root) * s } else { p };
        let (cam, fov) = if !idle {
            let (fc, cc) = &d.cams[self.view as usize];
            let f = self.frame.min(cc.frames as f32 - 1.0).max(0.0);
            let (fov, m) = (fc.sample(f)[0], cc.sample(f));
            (Mat4::from_cols_array(&[m[0], m[4], m[8], m[12], m[1], m[5], m[9], m[13], m[2], m[6], m[10], m[14], m[3], m[7], m[11], m[15]]), fov)
        } else {
            if refresh {
                let pts: Vec<Vec3> = [(&d.body, &bm, 0usize), (&d.head, &hm, 1)].iter().flat_map(|(m, mats, mi)| m.shapes.iter().flat_map(move |sh| dancer::skin_positions(sh, mats).into_iter().map(move |p| scaled(*mi, p)))).collect();
                posed.bounds = pts.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
            }
            let (lo, hi) = posed.bounds;
            let c = (lo + hi) / 2.0;
            let dist = (hi.y - lo.y).max(1.0) * 2.4;
            let eye = c + Vec3::new(self.orbit.sin() * dist, 0.0, self.orbit.cos() * dist);
            (glam::camera::rh::view::look_at_mat4(eye, c, Vec3::Y).inverse(), 30.0)
        };
        let proj = glam::camera::rh::proj::directx::perspective(fov.to_radians(), aspect, 0.5, 4000.0);
        // Lights ride on the camera (FUN_10266820).
        let rcam = Mat3::from_mat4(cam);
        let lights: Vec<Light> = LIGHTS
            .iter()
            .map(|(q, c)| {
                let frame = rcam * Mat3::from_quat(Quat::from_array(*q).normalize());
                Light { dir: frame.z_axis.normalize(), color: *c, frame }
            })
            .collect();
        let (eye, fwd) = (cam.w_axis.truncate(), -cam.z_axis.truncate().normalize());
        if refresh {
            // build_draws' key: each listed-triangle run in descending (P0 + P1 + P2 - 3 eye) . fwd.
            for (mi, (m, mats)) in [(&d.body, &bm), (&d.head, &hm)].into_iter().enumerate() {
                for (si, sh) in m.shapes.iter().enumerate().filter(|(_, sh)| sh.sorted) {
                    let pos: Vec<Vec3> = dancer::skin_positions(sh, mats).into_iter().map(|p| scaled(mi, p)).collect();
                    let key = |t: &[u32; 3]| (pos[t[0] as usize] + pos[t[1] as usize] + pos[t[2] as usize] - 3.0 * eye).dot(fwd);
                    let mut tris = sh.tris.clone();
                    for &(a, n) in &sh.list_runs {
                        tris[a..a + n].sort_by(|x, y| key(y).total_cmp(&key(x)));
                    }
                    sorted.push((mi, si, tris));
                }
            }
            posed.sorted_at = Some(tick);
        }
        let m = Vec3::ONE - self.backdrop.shadow_c;
        r.shadow = Some(crate::render::dancer::Shadow { light: crate::render::dancer::SHADOW_LIGHT, mult: m.to_array() });
        let frame = crate::render::dancer::GFrame { view_proj: proj * cam.inverse(), eye, lights: &lights, joints: [&bm, &hm], root, scale: s, sorted };
        posed.gpu.update(gpu, r, &frame, false);
        self.pose_time += t0.elapsed();
        let camera = zone_camera(cam, fov.to_radians());
        zone.render(gpu, encoder, view, depth, &camera, aspect, &[]);
        if let Some(fx) = &mut self.effects {
            fx.draw(gpu, format, encoder, target, depth, zone, &camera, aspect, alive);
        }
        if self.show_dancer {
            posed.gpu.draw(r, encoder, view, depth);
        }
    }
}

/// Step menus (0x30 layouts) and their item counts, race .. nation.
pub const STEPS: [(&str, usize); 7] = [
    ("menu    chmkrace", 8),
    ("menu    chmkface", 8),
    ("menu    chmkhair", 2),
    ("menu    chmksize", 3),
    ("menu    chmkjobs", 6),
    ("menu    chmkname", 0),
    ("menu    chmktown", 3),
];

/// worldsel's visible rows (FUN_101f6700(0xe)).
pub const WORLD_ROWS: usize = 14;

/// Retail's creator step 6 (FUN_10239720) over our name step: the world list (worldsel), then the Gold World Pass
/// (chmkpass).
pub struct WorldStep {
    /// World names in the list's order.
    pub rows: Vec<String>,
    pub cursor: usize,
    pub top: usize,
    /// Phase 1: the pass (digits, at most 10) and its caret.
    pub pass: Option<(String, usize)>,
    /// chmkpass +0x34, the caret's frame count (30 a second, two a frame while the pass is empty), and its last tick.
    pub blink: f32,
    last: Instant,
}

/// What an input event did on the world step.
pub enum WorldAct {
    None,
    Sound(crate::audio::Ui),
    /// Escape on the list: back to the name.
    Back,
    /// Enter on the pass: the checks.
    Check,
}

impl WorldStep {
    /// FUN_1023d6c0 mode 0: worlds 1..=200 not "unknown", in a random order (new on every open); none, no list.
    pub fn new(worlds: &[(u32, String)]) -> Option<Self> {
        let mut rows: Vec<String> = worlds.iter().filter(|(id, n)| (1..=200).contains(id) && n != "unknown").map(|(_, n)| n.clone()).collect();
        if rows.is_empty() {
            return None;
        }
        for i in (1..rows.len()).rev() {
            let mut b = [0u8; 4];
            let _ = getrandom::fill(&mut b);
            rows.swap(i, u32::from_le_bytes(b) as usize % (i + 1));
        }
        Some(Self { rows, cursor: 0, top: 0, pass: None, blink: 0.0, last: Instant::now() })
    }

    pub fn tick(&mut self) {
        let dt = self.last.elapsed().as_secs_f32().min(0.5);
        self.last = Instant::now();
        if let Some((p, _)) = &self.pass {
            self.blink += dt * if p.is_empty() { 60.0 } else { 30.0 };
        }
    }

    /// chmkserv's text: the cursor row's world (frozen while the pass is open).
    pub fn world(&self) -> &str {
        &self.rows[self.cursor]
    }

    pub fn view(&self) -> super::game_ui::WorldView<'_> {
        let pass = self.pass.as_ref().map(|(p, at)| (p.as_str(), *at, self.blink as u32 % 30 > 15));
        super::game_ui::WorldView { rows: &self.rows, cursor: self.cursor, top: self.top, pass }
    }

    /// Phase 0 worldsel (FUN_101f6c70, chmkserv FUN_10238a10), phase 1 chmkpass (FUN_10238d60).
    pub fn event(&mut self, ev: &egui::Event) -> WorldAct {
        use crate::audio::Ui;
        use egui::{Event, Key};
        if let Some((p, at)) = self.pass.as_mut() {
            match ev {
                // Input class 1 (FUN_101b6d50): digits only, at most 10.
                Event::Text(t) | Event::Paste(t) => {
                    for c in t.chars().filter(|c| c.is_ascii_digit()) {
                        if p.len() < 10 {
                            p.insert(*at, c);
                            *at += 1;
                        }
                    }
                }
                Event::Key { key, pressed: true, .. } => match key {
                    Key::Backspace if *at > 0 => {
                        *at -= 1;
                        p.remove(*at);
                    }
                    Key::ArrowLeft => *at = at.saturating_sub(1),
                    Key::ArrowRight => *at = (*at + 1).min(p.len()),
                    Key::Enter => return WorldAct::Check,
                    // The list again, same order (FUN_1023d6c0 mode 1); the cursor on row 1 (inf).
                    Key::Escape => {
                        self.pass = None;
                        (self.cursor, self.top) = (0, 0);
                        return WorldAct::Sound(Ui::Cancel);
                    }
                    _ => {}
                },
                _ => {}
            }
            return WorldAct::None;
        }
        match ev {
            Event::Key { key: Key::ArrowDown, pressed: true, .. } => self.down(),
            Event::Key { key: Key::ArrowUp, pressed: true, .. } => self.up(),
            Event::Key { key: Key::Enter, pressed: true, .. } => self.pick(),
            Event::Key { key: Key::Escape, pressed: true, .. } => WorldAct::Back,
            _ => WorldAct::None,
        }
    }

    /// Enter on a row: chmkpass opens empty (SE 0002).
    pub fn pick(&mut self) -> WorldAct {
        self.pass = Some((String::new(), 0));
        self.blink = 0.0;
        WorldAct::Sound(crate::audio::Ui::Select)
    }

    /// The layout links wrap rows 1 <-> 14; the list clamps a move past its count back, silently (FUN_101f6c70).
    fn down(&mut self) -> WorldAct {
        if self.cursor + 1 < self.rows.len() {
            self.cursor += 1;
            if self.cursor >= self.top + WORLD_ROWS {
                self.top += 1;
            }
        } else if self.top == 0 && self.cursor == WORLD_ROWS - 1 {
            self.cursor = 0;
        } else {
            return WorldAct::None;
        }
        WorldAct::Sound(crate::audio::Ui::Cursor)
    }

    fn up(&mut self) -> WorldAct {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.top = self.top.min(self.cursor);
        } else {
            // Row 1's link goes to row 14, clamped to the count (one world: back on row 1, the sound kept).
            self.cursor = WORLD_ROWS.min(self.rows.len()) - 1;
        }
        WorldAct::Sound(crate::audio::Ui::Cursor)
    }

    /// The modern list's row `k` (every world shown): the cursor there, kept in retail's window; true when it moved.
    pub fn point(&mut self, k: usize) -> bool {
        let moved = k < self.rows.len() && k != self.cursor;
        if moved {
            self.cursor = k;
            self.top = self.top.min(k).max((k + 1).saturating_sub(WORLD_ROWS));
        }
        moved
    }

    /// The mouse on visible row `i`: the cursor there; true when it moved.
    pub fn hover(&mut self, i: usize) -> bool {
        let c = self.top + i;
        let moved = c < self.rows.len() && c != self.cursor;
        if moved {
            self.cursor = c;
        }
        moved
    }
}

/// What the modern creator's mouse did (modern/lobby.rs creator_screen): the step's row, a world row, Yes (true) or No
/// on the confirm, each (hovered, clicked); the ok window's OK; Enter on the name, world or pass; Num+'s random name.
#[derive(Default)]
pub struct Pointer {
    pub row: (Option<usize>, Option<usize>),
    pub world: (Option<usize>, Option<usize>),
    pub yes: (Option<bool>, Option<bool>),
    pub ok: bool,
    pub enter: bool,
    pub random: bool,
}

/// The creator as the modern panels see it: the help line (or the status, `status`), the ok window's text, the
/// confirm's (text, Yes lit), and whether input is taken (no request under way).
pub struct Page<'a> {
    pub view: &'a super::game_ui::CreatorView<'a>,
    pub help: String,
    pub status: bool,
    pub ok: Option<String>,
    pub confirm: Option<(String, bool)>,
    pub free: bool,
}

/// The help line of the creator's current state (lobyhelp 0x19 / 0x22 on the world step, FUN_10241270; 0x22 stays
/// through the checks, inf).
pub fn help_text(gu: &super::game_ui::GameUi, v: &super::game_ui::CreatorView, status: Option<&str>) -> String {
    match (status, &v.world) {
        (Some(s), _) => s.to_string(),
        (None, Some(w)) => gu.lobby_msg(8, if w.pass.is_some() { 0x71 } else { 0x70 }, ""),
        (None, None) if v.step == 5 && v.name_hidden => gu.lobby_msg(8, 0x71, ""),
        _ => gu.creator_help(v.step),
    }
}

/// FUN_102384d0's random name (Num+ on the name step).
pub fn roll_name(index: &crate::dat::DatIndex) -> Option<String> {
    let mut b = [0u8; 4];
    random_name(index, || {
        let _ = getrandom::fill(&mut b);
        u32::from_le_bytes(b) & 0x7fff
    })
}

/// The modern creator's 2D layer: the nation's emblem from the nation step on (the panels draw the rest).
pub fn draw_modern(gu: &mut super::game_ui::GameUi, v: &super::game_ui::CreatorView, sw: f32, sh: f32) {
    if v.step >= 6 {
        gu.bare_window(&format!("menu    nation{}", v.nation.min(2) + 1), sw, sh);
    }
}

/// The creator's 2D layer (after `gu.clear()`): help line, the step menu with its choice highlighted,
/// then the race title, stats, name entry or nation emblem. Returns the menu's item rects, or the world list's rows.
pub fn draw_ui(gu: &mut super::game_ui::GameUi, v: &super::game_ui::CreatorView, sw: f32, sh: f32, status: Option<&str>, dt: Option<f32>) -> Vec<[f32; 4]> {
    let help = help_text(gu, v, status);
    gu.help_line(&help, sw, sh, if status.is_some() { super::game_ui::YELLOW } else { super::game_ui::WHITE }, dt);
    let sel = match v.step {
        0 => v.race,
        1 => v.face,
        2 => v.hair,
        3 => v.size,
        4 => v.job,
        6 => v.nation,
        _ => 0,
    };
    let mut rects = match v.step {
        5 if v.name_hidden => Vec::new(),
        // chmkserv's draw hides every chmkname child (FUN_1011a3d0).
        5 if v.world.is_some() => {
            gu.window_bare_frame(STEPS[5].0, sw, sh);
            Vec::new()
        }
        s if s < 7 => gu.menu(STEPS[s].0, sw, sh, sel),
        _ => Vec::new(),
    };
    gu.creator_overlay(sw, sh, v);
    if let Some(w) = &v.world {
        rects = gu.world_step(w, sw, sh);
    }
    rects
}

/// `creator-shot --screen`: one creator screen (zone, dancer, 2D layer) offscreen.
pub struct ScreenShot {
    pub race: usize,
    pub face: usize,
    pub hair: usize,
    pub size: usize,
    pub job: usize,
    pub nation: usize,
    pub step: usize,
    pub name: String,
    /// The world step: "list", or "pass:DIGITS".
    pub world: Option<String>,
    /// Performance frame (30 per second) and view mode 0..2.
    pub frame: f32,
    pub view: u8,
    /// Seconds of help-line time before the shot (scroll position).
    pub help_time: f32,
    pub width: u32,
    pub height: u32,
    /// Also run this many frames at 60 Hz and print the time per frame.
    pub bench: u32,
}

pub fn screen_shot(shot: &ScreenShot, out: &std::path::Path) -> Result<()> {
    use super::game_ui::{CreatorView, GameUi, GameUiData};
    let cfg = crate::config::Config::load();
    let index = Arc::new(DatIndex::open(&cfg.ffxi_dir())?);
    let dll = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?;
    let keys = Arc::new(zone::Keys::from_ffximain(&dll)?);
    let mut gu = GameUi::new(GameUiData::load(&index, &dll)?);
    let mut cr = Creator::new(Backdrop::load(&index, &keys)?);
    cr.set_dancer(Dancer::load(&index, shot.race, shot.face)?);
    cr.hair = shot.hair;
    cr.view = shot.view;
    cr.seek(shot.frame);
    let (w, h) = (shot.width, shot.height);
    let gpu = Gpu::headless()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("creator shot"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let depth = crate::render::depth_view(&gpu.device, w, h);
    // A second of lobby effects first (the clouds), the performance held at --frame.
    for _ in 0..30 {
        if let Some(fx) = &mut cr.effects {
            fx.advance(1.0 / 30.0);
        }
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        cr.render(&gpu, format, &mut enc, (&view, Some(&target), [w, h]), &depth, w as f32 / h.max(1) as f32, None);
        gpu.queue.submit([enc.finish()]);
        let _ = gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        if let Some(fx) = &mut cr.effects {
            fx.after_submit(&gpu);
        }
    }
    // --world list | pass:DIGITS: the world step over the name step, one world.
    let ws = shot.world.as_deref().and_then(|w| {
        let mut s = WorldStep::new(&[(0x20, "Nameless".to_string())])?;
        if let Some(p) = w.strip_prefix("pass") {
            let p = p.trim_start_matches(':').to_string();
            s.pass = Some((p.clone(), p.len()));
            s.blink = 20.0;
        }
        Some(s)
    });
    let v = CreatorView { step: shot.step, race: shot.race, face: shot.face, hair: shot.hair, size: shot.size, job: shot.job, nation: shot.nation, name: &shot.name, world: ws.as_ref().map(|s| s.view()), name_hidden: false };
    // The nation step plays file 24 + nation's mov1 instead (FUN_1023ae20); --frame is its time at 30 a second.
    let mut fly = if shot.step == 6 {
        let mut f = super::flyover::Flyover::new(super::flyover::FlyoverData::load_file(&index, &keys, 24 + shot.nation as u32, "mov1")?).unfaded();
        for _ in 0..(shot.frame * 2.0) as u32 {
            f.advance(1.0 / 60.0);
        }
        Some(f)
    } else {
        None
    };
    let mut frame = |cr: &mut Creator, gu: &mut GameUi, dt: f32| {
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        match &mut fly {
            Some(f) => {
                f.render(&gpu, format, &mut enc, &view, &depth, [w, h], w as f32 / h.max(1) as f32);
            }
            None => cr.render(&gpu, format, &mut enc, (&view, Some(&target), [w, h]), &depth, w as f32 / h.max(1) as f32, None),
        }
        gu.clear();
        draw_ui(gu, &v, w as f32, h as f32, None, Some(dt));
        gu.render(&gpu, format, &mut enc, &view, [w, h], [w, h], true, 1.0, 1.0);
        enc
    };
    // Help-line time before the shot, in 30 Hz steps.
    for _ in 0..(shot.help_time * 30.0) as u32 {
        gu.clear();
        draw_ui(&mut gu, &v, w as f32, h as f32, None, Some(1.0 / 30.0));
        gu.end_frame(1.0 / 30.0);
    }
    if shot.bench > 0 {
        let (t0, pose0) = (std::time::Instant::now(), cr.pose_time);
        for _ in 0..shot.bench {
            cr.advance(1.0 / 60.0);
            let enc = frame(&mut cr, &mut gu, 1.0 / 60.0);
            gpu.queue.submit([enc.finish()]);
            gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None })?;
        }
        let per = t0.elapsed().as_secs_f64() * 1000.0 / shot.bench as f64;
        let pose = (cr.pose_time - pose0).as_secs_f64() * 1000.0 / shot.bench as f64;
        println!("bench: {} frames at 60 Hz: {per:.2} ms a frame (wall, with the GPU wait), posing {pose:.2} ms a frame", shot.bench);
        cr.seek(shot.frame);
    }
    let mut enc = frame(&mut cr, &mut gu, 0.0);
    let row = (w * 4).div_ceil(256) * 256;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    enc.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    gpu.queue.submit([enc.finish()]);
    super::save_capture(&gpu, &buf, [w, h], row, format, out, None, None)?;
    println!("wrote {}", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_camera_flips_y_and_z() {
        // Camera 0 of Hume M starts at (9.6, 3.8, -87.5) facing +z (dancer, y up): zone (0.96, -0.38, 8.75) facing -z.
        let cam = Mat4::from_rotation_translation(Quat::from_rotation_y(std::f32::consts::PI), Vec3::new(9.6, 3.8, -87.5));
        let c = zone_camera(cam, 0.6);
        assert!((c.eye - Vec3::new(0.96, -0.38, 8.75)).length() < 1e-4);
        assert!((c.target - c.eye - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-4);
        assert!((c.up - Vec3::NEG_Y).length() < 1e-4);
    }
}

/// The name step's random name (FUN_10235310, composed by FUN_10238330): a noun from file 0x7c07 (16-byte records,
/// rand % 981, grouped by length: weights 0x103873f8) and an adjective from 0x7c06 (class = floor(log2(rand % (2 ^
/// (11 - noun class) - 1))), 1 for 0; rand % count + offset, tables 0x10387424 / 0x10387450); adjective + noun, kept
/// only under 16 letters. `rand` stands in for the CRT's rand() (0..32767).
pub fn random_name(index: &crate::dat::DatIndex, mut rand: impl FnMut() -> u32) -> Option<String> {
    const WEIGHT: [u32; 11] = [2, 56, 149, 171, 173, 169, 127, 81, 34, 17, 2];
    const COUNT: [u32; 11] = [21, 74, 153, 220, 264, 317, 190, 99, 51, 12, 9];
    const OFFSET: [u32; 11] = [0, 21, 95, 248, 468, 732, 1049, 1239, 1338, 1389, 1401];
    let word = |file: &[u8], i: u32| -> Option<String> {
        let r = file.get(i as usize * 16..i as usize * 16 + 16)?;
        let s: String = r.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect();
        Some(s)
    };
    let nouns = index.read(0x7c07).ok()?;
    let adjectives = index.read(0x7c06).ok()?;
    let n = rand() % 981;
    let mut acc = 0;
    let class = WEIGHT.iter().position(|w| {
        acc += w;
        acc > n
    })?;
    let budget = 10 - class as u32;
    let r = rand() % ((1u32 << (budget + 1)) - 1);
    let k = if r == 0 { 1 } else { (r as f64).log2() as usize };
    let a = rand() % COUNT.get(k)? + OFFSET[k];
    let name = word(&adjectives, a)? + &word(&nouns, n)?;
    // FUN_1022dc60 on entry: first letter upper case, the rest lower.
    (name.len() < 16 && !name.is_empty()).then(|| name.chars().enumerate().map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() }).collect())
}

#[cfg(test)]
mod random_name_tests {
    #[test]
    fn names_from_the_install() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let mut seed = 12345u32;
        let mut rand = move || {
            seed = seed.wrapping_mul(214013).wrapping_add(2531011);
            (seed >> 16) & 0x7fff
        };
        let names: Vec<String> = (0..20).filter_map(|_| super::random_name(&index, &mut rand)).collect();
        assert!(names.len() >= 18, "{names:?}");
        for n in &names {
            assert!((3..16).contains(&n.len()) && n.chars().all(|c| c.is_ascii_alphabetic()), "{n}");
            assert!(n.chars().next().unwrap().is_ascii_uppercase());
        }
    }
}

/// Config-tool 0023 (Simple Character Creation): chdummy mode 0 (FUN_1022ea40 / FUN_1022eca0, cur FUN_10239260 /
/// FUN_102394c0) shows a 2-D portrait of (race, face, hair) in place of the dancer: file 39631 + face % 8 + (race + hair
/// x 8) x 8. A change waits out a 10-frame settle, fades the old picture out 8 a frame, loads, fades in 8 a frame; the
/// file's sprite (512 x 448 at the chdummy window's origin, 0, 0) is drawn with colour (a, a, a, a), linear.
pub struct Portrait {
    /// +0x1c / +0x20: the index wanted and the one being shown; +0x18 the state; +0x24 the colour level.
    want: Option<u32>,
    shown: Option<u32>,
    state: u32,
    level: i32,
    /// The picture up (+0x34): its image name and size; the next one loading.
    up: Option<(String, f32, f32)>,
    rx: Option<std::sync::mpsc::Receiver<Option<(u32, u32, u32, Vec<u8>)>>>,
    pending: Option<(String, u32, u32, Vec<u8>)>,
    ticks: f32,
    last: Option<std::time::Instant>,
}

impl Default for Portrait {
    /// FUN_10239480(0): level 0x80, nothing wanted or shown.
    fn default() -> Self {
        Portrait { want: None, shown: None, state: 0, level: 0x80, up: None, rx: None, pending: None, ticks: 0.0, last: None }
    }
}

/// A portrait file's picture: its sprite's cell cut from the image (cells address it in stock texels).
fn portrait_picture(index: &DatIndex, file: u32) -> Option<(u32, u32, Vec<u8>)> {
    let ui = crate::dat::ui::UiFile::parse(&index.read(file).ok()?).ok()?;
    let cell = ui.libs.first()?.sprites.first()?.cells.first()?.clone();
    let img = ui.images.get(&cell.texture)?;
    let (u, v, w, h) = (cell.uv[0] as u32, cell.uv[1] as u32, cell.size[0] as u32, cell.size[1] as u32);
    if u + w > img.width || v + h > img.height {
        return None;
    }
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in v..v + h {
        let at = ((y * img.width + u) * 4) as usize;
        rgba.extend_from_slice(&img.rgba[at..at + (w * 4) as usize]);
    }
    Some((w, h, rgba))
}

impl Portrait {
    /// The picture index of race 0..7, face 0..7, hair 0 / 1.
    pub fn index(race: usize, face: usize, hair: usize) -> u32 {
        (face % 8 + (race + hair * 8) * 8) as u32
    }

    /// One update per 30 Hz frame since the last call.
    pub fn update(&mut self, want: u32, index: Option<&Arc<DatIndex>>) {
        let now = std::time::Instant::now();
        let dt = self.last.map_or(0.0, |t| (now - t).as_secs_f32().min(0.1));
        self.last = Some(now);
        self.want = Some(want);
        self.ticks += dt * 30.0;
        while self.ticks >= 1.0 {
            self.ticks -= 1.0;
            self.frame(index);
        }
    }

    fn frame(&mut self, index: Option<&Arc<DatIndex>>) {
        // The settle: a change starts it; at its end a further change goes back to waiting.
        match self.state {
            0 if self.want != self.shown => (self.shown, self.state) = (self.want, 1),
            1..=9 => self.state += 1,
            10 => self.state = if self.want != self.shown { 0 } else { 0xb },
            _ => {}
        }
        match self.state {
            0xb if self.level != 0 => self.level -= 8,
            0xb => self.state += 1,
            0xc => {
                let (Some(index), Some(n)) = (index, self.shown) else { return };
                let (index, (tx, rx)) = (index.clone(), std::sync::mpsc::channel());
                std::thread::spawn(move || {
                    let _ = tx.send(portrait_picture(&index, 39631 + n).map(|(w, h, rgba)| (n, w, h, rgba)));
                });
                self.rx = Some(rx);
                self.state += 1;
            }
            0xd => {
                if let Some(got) = self.rx.as_ref().and_then(|r| r.try_recv().ok()) {
                    self.rx = None;
                    if let Some((n, w, h, rgba)) = got {
                        let name = format!("portrait:{n}");
                        self.up = Some((name.clone(), w as f32, h as f32));
                        self.pending = Some((name, w, h, rgba));
                    }
                    self.state += 1;
                }
            }
            0xe if self.level != 0x80 => self.level += 8,
            0xe => self.state += 1,
            0xf => self.state = 0,
            _ => {}
        }
    }

    /// The picture at the origin; the cell colour (0x80, 0x80, 0x80, 0xc0) times the level.
    pub fn draw(&mut self, gu: &mut super::game_ui::GameUi) {
        if let Some((name, w, h, rgba)) = self.pending.take() {
            gu.image(&name, w, h, rgba);
        }
        let Some((name, w, h)) = &self.up else { return };
        let k = self.level as f32 / 128.0;
        gu.image_rect(name, 0.0, 0.0, *w, *h, [k, k, k, (1.5 * k).min(1.0)]);
    }
}
