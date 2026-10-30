//! Character select (FUN_10238ea0) and delete (FUN_10230a10): zone 744 at 15:30 with the account's characters
//! on the radius-4 ring (FUN_10238580), the camera curving between them (FUN_10237a80 / FUN_10238250), the name
//! list (dbnamese, FUN_102372c0), Options (chfwin / chswin) and the prompts (ptc6yesn, ok).

use super::game_ui::{GameUi, WHITE};
use super::ground::Ground;
use crate::audio::Ui as Se;
use crate::dat::character::{Character as Model, Look, load_character};
use crate::dat::weather::{Lighting, ZoneWeather};
use crate::dat::{DatIndex, zone};
use crate::net::lobby::Character;
use crate::render::{ActorGpu, Camera, Gpu, ZoneRenderer};
use anyhow::Result;
use glam::{Mat4, Vec3};
use std::sync::Arc;

pub const SLOTS: usize = 16;
/// The lobby zone (retail FUN_10242cd0; 133 in the 2019 build). The creator draws over it too.
pub const ZONE: u32 = 744;
/// FUN_1023b360(15, 30).
const MINUTE: f32 = 15.5 * 60.0;
/// A lobby frame: FUN_100889d0(2) holds every frame to two 60 Hz ticks.
pub const FRAME: f32 = 1.0 / 30.0;
/// Head offset without a model point (DAT_1037c080..0b0) and the lift over the ring centre (DAT_1037c0c0).
const HEAD: Vec3 = Vec3::new(0.0, -1.0, 0.0);
const LIFT: Vec3 = Vec3::new(0.0, -2.0, 0.0);
/// FUN_102372c0 retries the camera move on the 150th frame without input (> 0x95).
const REFRESH: u32 = 0x95;
/// The skeleton point FUN_10237a80 aims at (actor vtable +0x1c4 with 7): the upper chest.
const POINT: usize = 7;

/// A slot of the ring: feet, the close and far eyes (1.4 / 2.8 toward the centre), point 7 minus the feet.
#[derive(Clone, Copy)]
struct Slot {
    feet: Vec3,
    near: Vec3,
    far: Vec3,
    point: Option<Vec3>,
}

/// Zone, ring and the models of the characters in each slot; loaded off the UI thread.
pub struct SelectData {
    scene: zone::ZoneScene,
    lighting: Option<Lighting>,
    centre: Vec3,
    slots: [Slot; SLOTS],
    models: Vec<Option<(Model, u8)>>,
    fx: Option<FxLibs>,
}

/// The global and zone effect libraries of a lobby scene, with what the effects engine reads files through.
pub struct FxLibs {
    global: crate::fx::lib::Library,
    zone: crate::fx::lib::Library,
    files: (Arc<DatIndex>, Arc<zone::Keys>),
}

impl FxLibs {
    /// Off the UI thread, with the zone DAT already read.
    pub fn load(index: &Arc<DatIndex>, keys: &Arc<zone::Keys>, zone_file: &[u8]) -> Option<Self> {
        let lib = crate::fx::lib::Library::load;
        let libs = index.read(0).and_then(|g| Ok((lib(0, g)?, lib(zone::file_id(ZONE), zone_file.to_vec())?)));
        libs.map(|(global, zone)| Self { global, zone, files: (index.clone(), keys.clone()) }).map_err(|e| log::warn!("lobby effects: {e:#}")).ok()
    }
}

/// The effects engine in a lobby scene (select, creator): the zone's generators and the weather 'suny' start with
/// the zone (FUN_10242cd0, FUN_10239720), the sky's clouds among them. Its own GPU side: ids are per registry.
pub struct LobbyFx {
    fx: crate::fx::Effects,
    gpu: Option<crate::render::effects::EffectsGpu>,
    /// Real time not yet given to the effects; the lobby clock.
    dt: f32,
    minute: f32,
}

impl LobbyFx {
    pub fn new(libs: FxLibs, minute: f32) -> Self {
        let mut fx = crate::fx::Effects::new();
        fx.set_keys(libs.files.1);
        fx.set_index(libs.files.0);
        fx.set_global(libs.global);
        fx.load_zone(libs.zone);
        fx.set_weather("suny", 0.0);
        Self { fx, gpu: None, dt: 0.0, minute }
    }

    pub fn advance(&mut self, dt: f32) {
        self.dt += dt;
    }

    /// Effect sounds since the last call, for the sound thread.
    pub fn take_sounds(&mut self) -> Vec<crate::fx::sound::SoundCmd> {
        std::mem::take(&mut self.fx.sound_cmds)
    }

    /// After the frame's submit: the effects' readback.
    pub fn after_submit(&mut self, gpu: &Gpu) {
        if let Some(eg) = &mut self.gpu {
            eg.after_submit(gpu, &mut self.fx);
        }
    }

    /// One frame over the drawn zone.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, gpu: &Gpu, format: wgpu::TextureFormat, encoder: &mut wgpu::CommandEncoder, target: Target, depth: &wgpu::TextureView, zone: &ZoneRenderer, camera: &Camera, aspect: f32, alive: Option<Vec<u32>>) {
        let (fx, (view, color_tex, bg)) = (&mut self.fx, target);
        let eg = self.gpu.get_or_insert_with(|| crate::render::effects::EffectsGpu::new(gpu, format));
        if fx.reg.light_keys != zone.light_keys {
            fx.reg.light_keys = zone.light_keys.clone();
        }
        eg.set_lights(&zone.lights);
        let dt = std::mem::take(&mut self.dt);
        let view_m = camera.view();
        let mut view_rot = view_m;
        view_rot.w_axis = glam::Vec4::W;
        let f = crate::fx::Frame {
            dt: (dt * 60.0).max(1e-3),
            seconds: dt,
            dayfrac: self.minute / 1440.0,
            view: view_m,
            view_rot,
            eye: camera.eye,
            draw_distance: 2000.0,
            area_light: zone.lighting.map_or(Vec3::ONE, |l| l.area),
            focal: 192.0 / (camera.fov_y * 0.5).tan(),
            moon_phase: 0,
            weekday: 0,
        };
        if let Some(alive) = alive {
            fx.set_alive(alive);
        }
        fx.tick(&f, &crate::fx::NoActors);
        eg.update(gpu, encoder, fx, &f);
        let (fog_color, fog) = zone.lighting.as_ref().map_or(([0.0; 3], [150.0, 600.0]), |l| (l.fog.to_array(), [l.fog_start, l.fog_end]));
        eg.draw(gpu, encoder, (view, depth, bg), color_tex, fx, (camera.proj(aspect), view_m, camera.eye), [crate::render::NEAR, crate::render::FAR], fog_color, fog);
    }
}

/// The scene's colour target, its texture (for distortion) and the background size.
pub type Target<'a> = (&'a wgpu::TextureView, Option<&'a wgpu::Texture>, [u32; 2]);

/// Facing the ring centre (model +x toward it), scaled by the model's size.
fn placement(feet: Vec3, scale: f32) -> Mat4 {
    let f = Vec3::new(-feet.x, 0.0, -feet.z).normalize_or(Vec3::X);
    Mat4::from_cols(f.extend(0.0), Vec3::Y.extend(0.0), f.cross(Vec3::Y).extend(0.0), feet.extend(1.0)) * Mat4::from_scale(Vec3::splat(scale))
}

/// Skeleton point `k` in world space at rest: bone rest transform x point offset, then `place` (as attach_point).
pub fn rest_point(model: &Model, k: usize, place: Mat4) -> Option<Vec3> {
    let (bone, off) = model.skeleton.points.get(k)?;
    let rest = crate::dat::character::pose(&model.skeleton, &[]);
    let (q, t) = rest.get(*bone as usize)?;
    Some(place.transform_point3(*t + *q * *off))
}

impl SelectData {
    /// `looks[i]`: the character in list slot i and its size, if any.
    pub fn load(index: &Arc<DatIndex>, keys: &Arc<zone::Keys>, looks: &[Option<(Look, u8)>]) -> Result<Self> {
        let file = index.read(zone::file_id(ZONE))?;
        let scene = zone::load_scene(keys, &file)?;
        let fx = FxLibs::load(index, keys, &file);
        let lighting = ZoneWeather::parse(&file).ok().and_then(|w| w.at("suny", MINUTE)).map(|e| Lighting::from_env(&e, MINUTE));
        let ground = Ground::build(&scene);
        let models: Vec<Option<(Model, u8)>> = looks
            .iter()
            .map(|l| l.as_ref().and_then(|(look, size)| load_character(index, look).map_err(|e| log::warn!("select model: {e:#}")).ok().map(|m| (m, *size))))
            .collect();
        // FUN_10238580: the centre and (4, 0, 0) turned by pi/8 per slot, each dropped onto the ground.
        let centre = Vec3::new(0.0, ground.settle(0.0, 0.0, -1.0).unwrap_or(0.0), 0.0);
        let slots = std::array::from_fn(|i| {
            let a = std::f32::consts::PI / 8.0 * (i + 1) as f32;
            let (x, z) = (4.0 * a.cos(), -4.0 * a.sin());
            let feet = Vec3::new(x, ground.settle(x, z, -1.0).unwrap_or(0.0), z);
            let out = Vec3::new(x, 0.0, z).normalize() * 1.4;
            let point = models.get(i).and_then(Option::as_ref).and_then(|(m, size)| rest_point(m, POINT, placement(feet, m.scale(*size)))).map(|p| p - feet);
            Slot { feet, near: feet - out, far: feet - out * 2.0, point }
        });
        Ok(Self { scene, lighting, centre, slots, models, fx })
    }
}

/// A camera curve (FUN_10238250): control points 2 from - mid, mid, 2 to - mid as a uniform quadratic B-spline,
/// stepped so it moves an arc of L / 75 a frame (L = |P0P1| + |P1P2|; the modelled arc is L / 2).
#[derive(Clone, Copy, Debug)]
struct Curve {
    p: [Vec3; 3],
    t: f64,
}

impl Curve {
    fn new(from: Vec3, mid: Vec3, to: Vec3) -> Self {
        Self { p: [from * 2.0 - mid, mid, to * 2.0 - mid], t: 0.0 }
    }

    fn at(&self) -> Vec3 {
        let t = self.t as f32;
        let u = 1.0 - t;
        self.p[0] * (0.5 * u * u) + self.p[1] * (0.5 + t * u) + self.p[2] * (0.5 * t * t)
    }

    /// One frame: solves a dt^2 + b dt = c for the arc s(t) = l0 t + a t^2.
    fn step(&mut self) {
        if self.t >= 1.0 {
            return;
        }
        let l0 = (self.p[0] - self.p[1]).length() as f64;
        let l = l0 + (self.p[2] - self.p[1]).length() as f64;
        let a = 0.5 * l - l0;
        let b = l0 + 2.0 * a * self.t;
        let c = l / 75.0;
        if a.abs() >= 0.01 {
            self.t += ((b * b + 4.0 * a * c).max(0.0).sqrt() - b) / (2.0 * a);
        } else if b > 0.0 {
            self.t += c / b;
        }
        self.t = self.t.min(1.0);
    }
}

pub struct SelectScene {
    data: SelectData,
    zone: Option<ZoneRenderer>,
    actors: Vec<Option<ActorGpu>>,
    /// The slot the camera is on (+0x14) and the slots whose point 7 has been read (E+0xa8).
    cam_slot: usize,
    latched: [bool; SLOTS],
    eye: Vec3,
    target: Vec3,
    /// Target and eye curves (+0xb38, +0xb68).
    curves: Option<(Curve, Curve)>,
    /// Frames since the last input (+0x28) and frame time owed.
    idle: u32,
    acc: f32,
    time: f32,
    effects: Option<LobbyFx>,
}

impl SelectScene {
    pub fn new(mut data: SelectData) -> Self {
        let s = data.slots[0];
        let effects = data.fx.take().map(|l| LobbyFx::new(l, MINUTE));
        let mut scene = Self {
            data,
            zone: None,
            actors: Vec::new(),
            cam_slot: 0,
            latched: [false; SLOTS],
            eye: s.far + HEAD,
            target: s.feet + HEAD,
            curves: None,
            idle: 0,
            acc: 0.0,
            time: 0.0,
            effects,
        };
        scene.reset();
        scene
    }

    /// FUN_10238580 on opening: slot 0's far full-body shot whatever is there, nothing read yet.
    pub fn reset(&mut self) {
        let s = self.data.slots[0];
        (self.cam_slot, self.latched, self.curves, self.idle, self.acc) = (0, [false; SLOTS], None, 0, 0.0);
        (self.eye, self.target) = (s.far + HEAD, s.feet + HEAD);
    }

    fn used(&self, i: usize) -> bool {
        self.data.models.get(i).is_some_and(Option::is_some)
    }

    /// Point 7 minus the feet (FUN_100d23f0), latched once read; (0, -1, 0) and false without it.
    fn head(&mut self, i: usize) -> (Vec3, bool) {
        self.latched[i] = self.data.slots[i].point.is_some();
        self.data.slots[i].point.map_or((HEAD, false), |p| (p, true))
    }

    /// FUN_10237a80. `force` (the refresh) only while the camera's own slot has no point read yet.
    fn glide(&mut self, cursor: usize, force: bool) {
        let (old, new) = (self.cam_slot, cursor % SLOTS);
        if (!force && old == new) || (force && (!self.used(old) || self.latched[old])) {
            return;
        }
        let oh = if self.used(old) { self.head(old).0 } else { HEAD };
        let (nh, valid) = if self.used(new) { self.head(new) } else { (HEAD, false) };
        let (a, b, c) = (self.data.slots[old], self.data.slots[new], self.data.centre);
        let d = new as i32 - old as i32;
        let close = matches!(d.abs(), 0 | 1 | 15);
        let eye_mid = if close { (a.far + b.far + oh + nh) * 0.5 } else { c + LIFT };
        let eye_end = if valid { b.near } else { b.far } + nh;
        // Far apart: the bare feet of the slot halfway; both pulled 20 % toward the centre.
        let x = if close { (self.target + b.feet + nh) * 0.5 } else { self.data.slots[old.min(new) + d.unsigned_abs() as usize / 2].feet };
        let target_mid = c + (x - c) * 0.8;
        self.curves = Some((Curve::new(self.target, target_mid, b.feet + nh), Curve::new(self.eye, eye_mid, eye_end)));
        self.cam_slot = new;
    }

    /// Any message to the list zeroes the refresh count (FUN_102378f0).
    pub fn input(&mut self) {
        self.idle = 0;
    }

    /// The cursor moved: the camera follows only onto a slot with a character (cases 1..4 and 9).
    pub fn cursor_moved(&mut self, cursor: usize) {
        self.idle = 0;
        if self.used(cursor) {
            self.glide(cursor, false);
        }
    }

    /// Real time in whole lobby frames, whatever the drawing rate.
    pub fn advance(&mut self, dt: f32, cursor: usize) {
        self.time += dt;
        if let Some(fx) = &mut self.effects {
            fx.advance(dt);
        }
        self.acc = (self.acc + dt).min(FRAME * 8.0);
        while self.acc >= FRAME {
            self.acc -= FRAME;
            self.frame(cursor);
        }
    }

    /// FUN_102372c0's count, then FUN_10238250.
    fn frame(&mut self, cursor: usize) {
        self.idle += 1;
        if self.idle > REFRESH {
            if self.used(cursor) {
                self.glide(cursor, true);
            }
            self.idle = 0;
        }
        if let Some((t, e)) = &mut self.curves {
            t.step();
            e.step();
            (self.target, self.eye) = (t.at(), e.at());
        }
    }

    pub fn camera(&self) -> (Vec3, Vec3) {
        (self.eye, self.target)
    }

    pub fn effects(&mut self) -> Option<&mut LobbyFx> {
        self.effects.as_mut()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(&mut self, gpu: &Gpu, format: wgpu::TextureFormat, encoder: &mut wgpu::CommandEncoder, target: Target, depth: &wgpu::TextureView, aspect: f32, alive: Option<Vec<u32>>) {
        let view = target.0;
        let zone = self.zone.get_or_insert_with(|| ZoneRenderer::new(gpu, &self.data.scene, format));
        zone.lighting = self.data.lighting;
        if self.actors.len() != self.data.models.len() {
            self.actors = self.data.models.iter().map(|m| m.as_ref().map(|(model, _)| zone.new_actor(gpu, model))).collect();
        }
        // Standard PC actors in their default idle, facing the ring centre.
        for (i, (m, a)) in self.data.models.iter().zip(self.actors.iter_mut()).enumerate() {
            let (Some((model, size)), Some(actor)) = (m, a) else { continue };
            let ticks = self.time * 60.0 + i as f32 * 17.0;
            let anims: Vec<_> = model.find_animations("idl").into_iter().map(|x| (x, crate::motion::loop_frame(ticks * x.speed * 0.5, x.frames))).collect();
            let mesh = crate::render::character::pose_character(model, &anims);
            let feet = self.data.slots[i % SLOTS].feet;
            zone.update_actor(gpu, actor, &mesh, placement(feet, model.scale(*size)));
        }
        let camera = Camera { eye: self.eye, target: self.target, up: Vec3::NEG_Y, fov_y: crate::dat::title::fov_y(350.0) };
        let actors: Vec<&ActorGpu> = self.actors.iter().flatten().collect();
        zone.render(gpu, encoder, view, depth, &camera, aspect, &actors);
        if let Some(fx) = &mut self.effects {
            fx.draw(gpu, format, encoder, target, depth, zone, &camera, aspect, alive);
        }
    }
}

/// The list as FUN_102333b0 fills it: named characters of status 1 (available) or 2 (cannot be used) in packet
/// order, empty slots taking no entry. Content IDs purchased counts every status-1 slot, registered the named ones.
#[derive(Clone, Debug, Default)]
pub struct List {
    pub entries: Vec<usize>,
    pub purchased: u32,
    pub registered: u32,
}

impl List {
    pub fn new(chars: &[Character]) -> Self {
        let mut l = List::default();
        for (i, c) in chars.iter().enumerate().take(SLOTS).filter(|(_, c)| matches!(c.status, 1 | 2)) {
            if c.is_empty_slot() {
                l.purchased += (c.status == 1) as u32;
                continue;
            }
            l.entries.push(i);
            if c.status == 1 {
                l.purchased += 1;
                l.registered += 1;
            }
        }
        l
    }

    pub fn get<'a>(&self, chars: &'a [Character], slot: usize) -> Option<&'a Character> {
        chars.get(*self.entries.get(slot)?)
    }
}

/// A ptc6yesn prompt (FUN_10233b50 / FUN_102397c0 / FUN_10239700 types).
#[derive(Clone, Debug, PartialEq)]
pub enum Prompt {
    /// Type 0xd, group 10 0x52.
    Login,
    /// Type 4: group 10 0x29 and group 8 0xae.
    Delete,
    /// Type 10: group 0x1b 6.
    Reset,
    /// Types 0xb (save) / 0xc (load), chswin slot 0 server, 1..4 HDD A..D.
    Settings { save: bool, slot: u16 },
    /// Type 5 for a rename: group 8 0xaf and 0xb0.
    Register(String),
}

/// Who has the input on the select / delete screen.
#[derive(Clone, Debug, PartialEq)]
pub enum Focus {
    List,
    /// chfwin: child 1 load, 2 save (help 0x31 / 0x32).
    Options(u16),
    /// chswin in mode 0 load / 1 save (FUN_1023a8f0), child 1..5.
    Slots(u8, u16),
    /// The prompt and its cursor (true on Yes).
    Prompt(Prompt, bool),
    /// menu ok with its text, then where the input goes.
    Ok(String, Box<Focus>),
    /// chmkname for a rename (help 0x18 = group 0x1b 4).
    Rename(String),
    /// Ours (the modern layer): the Settings button under the list, its stop for the keys.
    Button,
}

pub enum Action {
    Login(usize),
    Delete(usize),
    Rename(usize, String),
    Back,
}

#[derive(Default)]
pub struct Out {
    pub action: Option<Action>,
    pub sounds: Vec<Se>,
    /// The list cursor moved there (camera follow), or some other input reached the list.
    pub moved: Option<usize>,
    pub input: bool,
}

pub struct Input<'a> {
    pub events: &'a [egui::Event],
    pub pointer: Option<(f32, f32)>,
    pub moved: bool,
    pub clicked: bool,
    pub busy: bool,
    /// The modern layer draws the screen and takes the mouse (View, Pointer): no retail draw.
    pub modern: bool,
}

/// A character row as the modern layer draws it: the name in its colour, then World / Race / Job / Level / Area.
pub struct RowView {
    pub name: String,
    pub tint: u32,
    pub info: [String; 5],
}

/// The select (or delete) screen as the modern layer draws it.
pub struct View {
    pub rows: Vec<RowView>,
    /// The info's labels (group 1 0..4).
    pub labels: [String; 5],
    pub cursor: usize,
    pub list_focus: bool,
    pub deleting: bool,
    pub help: String,
    pub counts: [String; 3],
    /// chfwin's rows and cursor (Load / Save), chswin's (the slots), the prompt (text, Yes lit), the ok window's text,
    /// the name being entered.
    pub options: Option<(Vec<String>, u16)>,
    pub slots: Option<(Vec<String>, u16)>,
    pub prompt: Option<(String, bool)>,
    pub ok: Option<String>,
    pub rename: Option<String>,
    /// The Settings button has the keys.
    pub button: bool,
}

/// What the modern layer's mouse did: a list row hovered / clicked, a window child (chfwin's, chswin's, the prompt's
/// Yes 1 / No 2, the ok window's 1) hovered / clicked, the options asked for (as the right button on the list).
#[derive(Default)]
pub struct Pointer {
    pub row: (Option<usize>, Option<usize>),
    pub child: (Option<u16>, Option<u16>),
    pub options: bool,
}

/// Mouse targets of the last draw.
#[derive(Default)]
struct Rects {
    list: Vec<[f32; 4]>,
    list_area: [f32; 4],
    children: Vec<(u16, [f32; 4])>,
}

pub struct SelectUi {
    /// DAT_10623afc: the list child - 1; FUN_102371c0 starts it at 0.
    pub cursor: usize,
    pub focus: Focus,
    rects: Rects,
    /// When a used row was pressed (the generic handler's window +0x5e = 9; empty rows get none).
    press: Option<std::time::Instant>,
    /// The modern layer draws the screen (its Settings button is a stop under the list).
    modern: bool,
}

impl Default for SelectUi {
    fn default() -> Self {
        Self { cursor: 0, focus: Focus::List, rects: Rects::default(), press: None, modern: false }
    }
}

fn inside(r: &[f32; 4], p: (f32, f32)) -> bool {
    p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3]
}

/// printf for the client's formats: %s, %c and %d with an optional width ("%2d", "%04d").
pub fn printf(fmt: &str, args: &[&str]) -> String {
    let (mut out, mut it, mut chars) = (String::new(), args.iter(), fmt.chars().peekable());
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = String::new();
        while let Some(&d) = chars.peek().filter(|d| d.is_ascii_digit()) {
            spec.push(d);
            chars.next();
        }
        match chars.next() {
            Some('s' | 'c') => out.push_str(it.next().copied().unwrap_or("")),
            Some('d') => {
                let v = it.next().copied().unwrap_or("0");
                let w: usize = spec.parse().unwrap_or(0);
                out.push_str(&if spec.starts_with('0') { format!("{v:0>w$}") } else { format!("{v:>w$}") });
            }
            Some(o) => out.push(o),
            None => out.push('%'),
        }
    }
    out
}

impl SelectUi {
    /// One frame: keys, the draw, then the mouse over what was drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn step(&mut self, gu: &mut GameUi, input: &Input, list: &List, chars: &[Character], deleting: bool, w: f32, h: f32, status: Option<(&str, [u8; 4])>) -> Out {
        let mut out = Out::default();
        self.cursor = self.cursor.min(SLOTS - 1);
        self.modern = input.modern;
        for ev in input.events {
            match ev {
                egui::Event::Key { key, pressed: true, modifiers, .. } => self.key(gu, *key, *modifiers, list, chars, deleting, input.busy, &mut out),
                // egui-winit may turn Ctrl+C into a copy command.
                egui::Event::Copy => self.key(gu, egui::Key::C, egui::Modifiers::CTRL, list, chars, deleting, input.busy, &mut out),
                egui::Event::Text(t) => {
                    if let Focus::Rename(name) = &mut self.focus {
                        name.extend(t.chars().filter(|c| c.is_ascii_alphabetic()));
                        name.truncate(15);
                        // FUN_1022dc60: first letter upper case, the rest lower.
                        *name = name.chars().enumerate().map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() }).collect();
                    }
                }
                egui::Event::PointerButton { button: egui::PointerButton::Secondary, pressed: true, pos, .. } => {
                    let p = input.pointer.unwrap_or((pos.x, pos.y));
                    self.right_click(p, list, deleting, &mut out);
                }
                _ => {}
            }
        }
        if input.modern {
            gu.clear();
            self.rects = Rects::default();
            return out;
        }
        self.rects = draw(gu, self, list, chars, deleting, w, h, status);
        if let Some(p) = input.pointer {
            self.mouse(gu, p, input, list, chars, deleting, &mut out);
        }
        out
    }

    fn used(&self, list: &List, slot: usize) -> bool {
        slot < list.entries.len()
    }

    /// Moves the cursor of the focused window along its layout's links (up, down, right, left = links 0, 1, 4, 5).
    fn link(gu: &GameUi, menu: &str, child: u16, key: egui::Key) -> Option<u16> {
        let k = match key {
            egui::Key::ArrowUp => 0,
            egui::Key::ArrowDown => 1,
            egui::Key::ArrowRight => 4,
            egui::Key::ArrowLeft => 5,
            _ => return None,
        };
        gu.menu_links(menu, child).map(|l| l[k] as u16).filter(|&c| c != 0 && c != 0xff)
    }

    #[allow(clippy::too_many_arguments)]
    fn key(&mut self, gu: &GameUi, key: egui::Key, mods: egui::Modifiers, list: &List, chars: &[Character], deleting: bool, busy: bool, out: &mut Out) {
        use egui::Key::*;
        match self.focus.clone() {
            // Modern: the Settings button is the stop after the last character (Down from it, Up from the first).
            Focus::List if self.modern && !deleting && !list.entries.is_empty() && (key == ArrowDown && self.cursor + 1 >= list.entries.len() || key == ArrowUp && self.cursor == 0) => {
                out.input = true;
                out.sounds.push(Se::Cursor);
                self.focus = Focus::Button;
            }
            Focus::Button => {
                out.input = true;
                match key {
                    ArrowUp | ArrowDown => {
                        let to = if key == ArrowUp { list.entries.len().saturating_sub(1) } else { 0 };
                        self.focus = Focus::List;
                        self.move_cursor(to, out);
                        out.sounds.push(Se::Cursor);
                    }
                    Enter | Plus => self.open_options(list, out),
                    Escape => {
                        out.sounds.push(Se::Cancel);
                        out.action = Some(Action::Back);
                    }
                    _ => {}
                }
            }
            Focus::List => {
                out.input = true;
                if let Some(c) = Self::link(gu, "menu    dbnamese", self.cursor as u16 + 1, key) {
                    self.move_cursor(c as usize - 1, out);
                } else if key == Enter && !busy {
                    self.confirm(gu, list, chars, deleting, out);
                } else if key == Escape {
                    out.sounds.push(Se::Cancel);
                    out.action = Some(Action::Back);
                } else if key == Plus && !deleting {
                    // Numpad + (key function 0x11, message 8; chfwin's caption "+ : Settings Management").
                    self.open_options(list, out);
                } else if key == C && (mods.ctrl || mods.alt) && !deleting && self.used(list, self.cursor) {
                    // Key function 0x3f (C with Ctrl or Alt), FUN_102370d0 -> FUN_10239700(10).
                    out.sounds.push(Se::Select);
                    self.focus = Focus::Prompt(Prompt::Reset, false);
                }
            }
            Focus::Options(k) => {
                if let Some(c) = Self::link(gu, "menu    chfwin", k, key) {
                    if c != k {
                        out.sounds.push(Se::Cursor);
                    }
                    self.focus = Focus::Options(c);
                } else if key == Enter {
                    // FUN_1023a760: child 1 opens chswin to load, 2 to save.
                    out.sounds.push(Se::Select);
                    self.focus = Focus::Slots((k == 2) as u8, 1);
                } else if key == Escape || key == Plus {
                    self.close_options(out);
                }
            }
            Focus::Slots(mode, k) => {
                if let Some(c) = Self::link(gu, "menu    chswin", k, key) {
                    if c != k {
                        out.sounds.push(Se::Cursor);
                    }
                    self.focus = Focus::Slots(mode, c);
                } else if key == Enter {
                    out.sounds.push(Se::Select);
                    self.focus = Focus::Prompt(Prompt::Settings { save: mode == 1, slot: k - 1 }, false);
                } else if key == Escape {
                    out.sounds.push(Se::Cancel);
                    self.focus = Focus::Options(mode as u16 + 1);
                }
            }
            // ptc6yesn (FUN_1023d050): Left / Right 0001 every time; Up / Down and Escape do nothing (in the modern layer
            // Escape is No, as every prompt there).
            Focus::Prompt(p, yes) => match key {
                ArrowLeft | ArrowRight => {
                    out.sounds.push(Se::Cursor);
                    self.focus = Focus::Prompt(p, !yes);
                }
                Enter => self.answer(gu, p, yes, list, out),
                Escape if self.modern => self.answer(gu, p, false, list, out),
                _ => {}
            },
            Focus::Ok(_, next) => {
                if key == Enter {
                    out.sounds.push(Se::Select);
                    self.focus = *next;
                }
            }
            Focus::Rename(mut name) => match key {
                Backspace => {
                    name.pop();
                    self.focus = Focus::Rename(name);
                }
                Enter if name.len() < 3 => {
                    // FUN_10233c80(0x6e): FFXI-3110.
                    self.focus = Focus::Ok(gu.error_text(3000 + 0x6e), Box::new(Focus::List));
                }
                Enter => {
                    out.sounds.push(Se::Select);
                    self.focus = Focus::Prompt(Prompt::Register(name), false);
                }
                Escape => {
                    out.sounds.push(Se::Cancel);
                    self.focus = Focus::List;
                }
                _ => {}
            },
        }
    }

    fn move_cursor(&mut self, to: usize, out: &mut Out) {
        if to != self.cursor {
            out.sounds.push(Se::Cursor);
        }
        self.cursor = to;
        out.moved = Some(to);
    }

    /// Case 5 on the list, then FUN_10238ea0's checks: "cannot use" (FFXI-3116), "must rename" (popup 0xda, then
    /// chmkname), else "Log in with ...?" with Yes highlighted. Deleting asks type 4 with No highlighted.
    fn confirm(&mut self, gu: &GameUi, list: &List, chars: &[Character], deleting: bool, out: &mut Out) {
        let Some(c) = list.get(chars, self.cursor) else {
            out.sounds.push(Se::Error);
            return;
        };
        out.sounds.push(Se::Select);
        self.press = Some(std::time::Instant::now());
        self.focus = if deleting {
            Focus::Prompt(Prompt::Delete, false)
        } else if c.status == 2 {
            Focus::Ok(gu.error_text(3000 + 0x74), Box::new(Focus::List))
        } else if c.rename_required {
            Focus::Ok(gu.lobby_msg(8, 0xda, ""), Box::new(Focus::Rename(String::new())))
        } else {
            Focus::Prompt(Prompt::Login, true)
        };
    }

    fn answer(&mut self, gu: &GameUi, p: Prompt, yes: bool, list: &List, out: &mut Out) {
        // 0002 for Yes and No alike.
        out.sounds.push(Se::Select);
        let idx = list.entries.get(self.cursor).copied();
        self.focus = Focus::List;
        match (p, yes, idx) {
            (Prompt::Login, true, Some(i)) => out.action = Some(Action::Login(i)),
            (Prompt::Delete, true, Some(i)) => out.action = Some(Action::Delete(i)),
            // FUN_101bb470 on the local settings file: this client keeps none, so retail's "not found" (0xe0).
            (Prompt::Reset, true, _) => self.focus = Focus::Ok(gu.lobby_msg(8, 0xe0, ""), Box::new(Focus::List)),
            // FUN_102398a0 needs the PlayOnline storage; without it retail's "not found" results (0xef / 0xf0).
            (Prompt::Settings { save, .. }, true, _) => self.focus = Focus::Ok(gu.lobby_msg(8, if save { 0xef } else { 0xf0 }, ""), Box::new(Focus::List)),
            (Prompt::Register(name), true, Some(i)) => out.action = Some(Action::Rename(i, name)),
            (Prompt::Register(name), false, _) => self.focus = Focus::Rename(name),
            _ => {}
        }
    }

    /// Case 8 (right button inside the list, FUN_102370d0): Options for a used slot, else a buzz.
    fn right_click(&mut self, p: (f32, f32), list: &List, deleting: bool, out: &mut Out) {
        match self.focus {
            Focus::List if !deleting && inside(&self.rects.list_area, p) => {
                if let Some(i) = self.rects.list.iter().position(|r| inside(r, p)) {
                    self.cursor = i;
                }
                self.open_options(list, out);
            }
            Focus::Options(_) => self.close_options(out),
            _ => {}
        }
    }

    /// Case 8 on the list: chfwin takes the focus (FUN_10157290, SE "0013") for a used slot, else a buzz.
    fn open_options(&mut self, list: &List, out: &mut Out) {
        out.input = true;
        if self.used(list, self.cursor) {
            out.sounds.push(Se::Close);
            self.focus = Focus::Options(1);
        } else {
            out.sounds.push(Se::Error);
        }
    }

    /// chfwin cases 6 / 8: focus back to the list (FUN_10157290), SE "0013", help 0x11.
    fn close_options(&mut self, out: &mut Out) {
        out.sounds.push(Se::Close);
        self.focus = Focus::List;
    }

    #[allow(clippy::too_many_arguments)]
    fn mouse(&mut self, gu: &GameUi, p: (f32, f32), input: &Input, list: &List, chars: &[Character], deleting: bool, out: &mut Out) {
        if !(input.moved || input.clicked) {
            return;
        }
        match self.focus.clone() {
            Focus::List => {
                let Some(i) = self.rects.list.iter().position(|r| inside(r, p)) else { return };
                gu.hover.set(true);
                out.input = true;
                if input.moved || i != self.cursor {
                    self.move_cursor(i, out);
                }
                if input.clicked && !input.busy {
                    self.confirm(gu, list, chars, deleting, out);
                }
            }
            focus => {
                let Some(&(c, _)) = self.rects.children.iter().find(|(_, r)| inside(r, p)) else { return };
                gu.hover.set(true);
                let enter = || egui::Key::Enter;
                match focus {
                    Focus::Options(k) if c != k => {
                        out.sounds.push(Se::Cursor);
                        self.focus = Focus::Options(c);
                    }
                    Focus::Slots(m, k) if c != k => {
                        out.sounds.push(Se::Cursor);
                        self.focus = Focus::Slots(m, c);
                    }
                    Focus::Prompt(pr, yes) if (c == 1) != yes => {
                        out.sounds.push(Se::Cursor);
                        self.focus = Focus::Prompt(pr, c == 1);
                    }
                    _ => {}
                }
                if input.clicked {
                    self.key(gu, enter(), egui::Modifiers::NONE, list, chars, deleting, input.busy, out);
                }
            }
        }
    }

    /// The screen for the modern layer: every used row with its info, the windows over the list.
    pub fn view(&self, gu: &GameUi, list: &List, chars: &[Character], deleting: bool) -> View {
        let rows = (0..list.entries.len())
            .filter_map(|i| list.get(chars, i))
            .map(|c| {
                let sjis = |b: Vec<u8>| crate::dat::font::plain(&crate::dat::font::decode_sjis(&b));
                let info = [c.world.clone(), sjis(gu.lobby_msg_raw(6, (c.race as usize).saturating_sub(1), "")), sjis(gu.lobby_msg_raw(3, c.main_job as usize, "")), c.main_level.max(1).to_string(), sjis(gu.area_name(c.zone))];
                RowView { name: c.name.clone(), tint: name_tint(c), info }
            })
            .collect();
        let labels = std::array::from_fn(|k| crate::dat::font::plain(&crate::dat::font::decode_sjis(&gu.lobby_msg_raw(1, k, ""))));
        let more = list.purchased as i64 - list.registered as i64;
        let fmts = gu.count_formats();
        let counts = [printf(&fmts[0], &[&list.purchased.to_string()]), printf(&fmts[1], &[&list.registered.to_string()]), printf(&fmts[if more == 1 { 3 } else { 2 }], &[&more.to_string()])];
        let captions = |menu: &str, n: u16| (1..=n).map(|k| gu.child_caption(menu, k).unwrap_or_default()).collect::<Vec<_>>();
        let name = list.get(chars, self.cursor).map_or("", |c| c.name.as_str());
        View {
            rows,
            labels,
            cursor: self.cursor,
            list_focus: self.focus == Focus::List,
            deleting,
            help: self.help(gu, deleting),
            counts,
            options: match self.focus {
                Focus::Options(k) => Some((captions("menu    chfwin", 2), k)),
                Focus::Slots(..) | Focus::Prompt(Prompt::Settings { .. }, _) => Some((captions("menu    chfwin", 2), 0)),
                _ => None,
            },
            slots: if let Focus::Slots(_, k) = self.focus { Some((captions("menu    chswin", 5), k)) } else { None },
            prompt: if let Focus::Prompt(p, yes) = &self.focus { Some((prompt_text(gu, p, name), *yes)) } else { None },
            ok: if let Focus::Ok(t, _) = &self.focus { Some(t.clone()) } else { None },
            rename: if let Focus::Rename(n) = &self.focus { Some(n.clone()) } else { None },
            button: self.focus == Focus::Button,
        }
    }

    /// The modern layer's mouse, as the retail mouse: a row moves the cursor and a click on it is Enter; a window's
    /// child moves its cursor and a click is Enter; the options as the right button.
    pub fn pointer(&mut self, gu: &GameUi, p: Pointer, list: &List, chars: &[Character], deleting: bool, busy: bool) -> Out {
        let mut out = Out::default();
        if p.options {
            match self.focus {
                Focus::List | Focus::Button if !deleting => self.open_options(list, &mut out),
                Focus::Options(_) => self.close_options(&mut out),
                _ => {}
            }
            return out;
        }
        if matches!(self.focus, Focus::List | Focus::Button) && p.row.0.or(p.row.1).is_some() {
            self.focus = Focus::List;
        }
        if self.focus == Focus::List {
            for i in p.row.0.into_iter().chain(p.row.1).filter(|&i| i < list.entries.len()) {
                out.input = true;
                self.move_cursor(i, &mut out);
            }
            if p.row.1.is_some() && !busy {
                self.confirm(gu, list, chars, deleting, &mut out);
            }
            return out;
        }
        for c in p.child.0.into_iter().chain(p.child.1) {
            match self.focus.clone() {
                Focus::Options(k) if c != k => {
                    out.sounds.push(Se::Cursor);
                    self.focus = Focus::Options(c);
                }
                Focus::Slots(m, k) if c != k => {
                    out.sounds.push(Se::Cursor);
                    self.focus = Focus::Slots(m, c);
                }
                Focus::Prompt(pr, yes) if (c == 1) != yes => {
                    out.sounds.push(Se::Cursor);
                    self.focus = Focus::Prompt(pr, c == 1);
                }
                _ => {}
            }
        }
        if p.child.1.is_some() {
            self.key(gu, egui::Key::Enter, egui::Modifiers::NONE, list, chars, deleting, busy, &mut out);
        }
        out
    }

    fn help(&self, gu: &GameUi, deleting: bool) -> String {
        match &self.focus {
            Focus::Options(k) => gu.lobby_msg(8, 0x101 + *k as usize, ""),
            // chswin help: table 0x10331def[item + mode * 5] -> lobyhelp 0x33.. -> group 8 0x104..0x10d.
            Focus::Slots(m, k) => gu.lobby_msg(8, 0x103 + *m as usize * 5 + *k as usize, ""),
            Focus::Rename(_) => gu.lobby_msg(0x1b, 4, ""),
            _ => gu.lobby_msg(8, if deleting { 0x6a } else { 0x69 }, ""),
        }
    }
}

/// The prompt's text (FUN_10231f50 by type): its lines joined with line breaks.
fn prompt_text(gu: &GameUi, p: &Prompt, name: &str) -> String {
    let m = |g: u8, id: usize| gu.lobby_msg(g, id, "");
    match p {
        Prompt::Login => printf(&m(10, 0x52), &[name]),
        Prompt::Delete => format!("{}\n{}", printf(&m(10, 0x29), &[name]), m(8, 0xae)),
        Prompt::Reset => printf(&m(0x1b, 6), &[name]),
        // Lines 3 and 4 (the stored file's details, 0xee / 0xcb) need the PlayOnline storage: not drawn.
        Prompt::Settings { save, slot } => {
            let (first, server, hdd) = if *save { (0xc7, 0xec, 0xc8) } else { (0xc9, 0xed, 0xca) };
            let letter = ((b'@' + *slot as u8) as char).to_string();
            let second = if *slot == 0 { m(8, server) } else { printf(&m(8, hdd), &[&letter]) };
            format!("{}\n{second}", printf(&m(8, first), &[name]))
        }
        Prompt::Register(n) => register_text(gu, n),
    }
}

/// Type 5: `Register "Name"` / `and begin play?` (8 0xaf / 0xb0), for the rename and the creator.
pub(crate) fn register_text(gu: &GameUi, name: &str) -> String {
    format!("{}\n{}", printf(&gu.lobby_msg(8, 0xaf, ""), &[name]), gu.lobby_msg(8, 0xb0, ""))
}

/// Name colours (FUN_102374a0): grey when it cannot be used, yellow when it must be renamed.
fn name_tint(c: &Character) -> u32 {
    if c.status == 2 {
        0x8050_5050
    } else if c.rename_required {
        0x807f_7f00
    } else {
        0x8080_8080
    }
}

/// The screen as FUN_102372c0 and its windows draw it.
#[allow(clippy::too_many_arguments)]
fn draw(gu: &mut GameUi, st: &SelectUi, list: &List, chars: &[Character], deleting: bool, w: f32, h: f32, status: Option<(&str, [u8; 4])>) -> Rects {
    gu.clear();
    let mut rects = Rects::default();
    let help = st.help(gu, deleting);
    let (text, colour) = status.unwrap_or((&help, WHITE));
    gu.help_line(text, w, h, colour, None);
    // chfwin opens with the list (select only); unfocused it keeps no highlight (registry flag 0x80000).
    let options = if let Focus::Options(k) = st.focus { Some(k) } else { None };
    if !deleting && let Some(win) = gu.lobby_window("menu    chfwin", w, h, false, options, options.is_some(), &[]) {
        if options.is_some() {
            rects.children = win.children;
        }
    }
    let used: Vec<bool> = (0..SLOTS).map(|i| i < list.entries.len()).collect();
    let press = st.press.map(|t| (9.0 - t.elapsed().as_secs_f32() * 60.0).max(0.0));
    let (list_rects, area) = gu.select_list(w, h, st.cursor, &used, st.focus == Focus::List, press);
    gu.sprite_from("menu    lobbywin", 143, 1, 0.0, 0.0, 0x8080_8080);
    gu.frame_tint(0.0, -16.0, w, h + 32.0, 0x2080_8080);
    for (i, r) in list_rects.iter().enumerate() {
        if let Some(c) = list.get(chars, i) {
            // FUN_10241a10: the name sits at the row rect + (4, 2), the cursor row's at -1, -1 with its sprite (+1, +1
            // while pressed).
            let d = match (i == st.cursor, press) {
                (true, Some(p)) if p > 0.0 => 1.0,
                (true, _) => -1.0,
                _ => 0.0,
            };
            gu.shape_fit(&c.name, r[0] + 4.0 + d, r[1] + 2.0 + d, r[2], name_tint(c));
        }
    }
    if let Some(c) = list.get(chars, st.cursor) {
        info_panel(gu, c, h);
    }
    counts(gu, list, w);
    match &st.focus {
        Focus::Slots(_, k) => {
            if let Some(win) = gu.lobby_window("menu    chswin", w, h, false, Some(*k), true, &[]) {
                rects.children = win.children;
            }
        }
        Focus::Prompt(p, yes) => {
            let name = list.get(chars, st.cursor).map_or("", |c| c.name.as_str());
            let icon = !matches!(p, Prompt::Settings { .. });
            if let Some(c) = gu.yes_no_window(&prompt_text(gu, p, name), *yes, icon, w, h) {
                rects.children = c;
            }
        }
        Focus::Ok(text, _) => {
            if let Some(c) = gu.ok_window(text, w, h) {
                rects.children = vec![(1, c)];
            }
        }
        Focus::Rename(name) => {
            gu.menu("menu    chmkname", w, h, 0);
            gu.name_entry(name, true, w, h);
        }
        _ => {}
    }
    rects.list = list_rects;
    rects.list_area = area;
    rects
}

/// FUN_102374f0, in the lobbywin glyph font (FUN_102308d0): the name, then World / Race / Job / Level / Area
/// (group 1 ids 0..4) at x 16 and their values at x 60, 18 px apart, from y = H * 370 / 448.
fn info_panel(gu: &mut GameUi, c: &Character, h: f32) {
    let base = (h * 0.825_892_87).trunc();
    gu.script_text(c.name.as_bytes(), 16.0, base - 144.0);
    let values: [Vec<u8>; 5] = [
        crate::dat::font::encode_sjis(&c.world),
        gu.lobby_msg_raw(6, (c.race as usize).saturating_sub(1), ""),
        gu.lobby_msg_raw(3, c.main_job as usize, ""),
        c.main_level.max(1).to_string().into_bytes(),
        gu.area_name(c.zone),
    ];
    for (k, v) in values.iter().enumerate() {
        let y = base - 108.0 + 18.0 * k as f32;
        let label = gu.lobby_msg_raw(1, k, "");
        gu.script_text(&label, 16.0, y);
        gu.script_text(v, 60.0, y);
    }
}

/// FUN_102376b0 (English): three fontshp lines at (W - 240, 35 / 51 / 67).
fn counts(gu: &mut GameUi, list: &List, w: f32) {
    let more = list.purchased as i64 - list.registered as i64;
    let fmts = gu.count_formats();
    let lines = [printf(&fmts[0], &[&list.purchased.to_string()]), printf(&fmts[1], &[&list.registered.to_string()]), printf(&fmts[if more == 1 { 3 } else { 2 }], &[&more.to_string()])];
    for (k, l) in lines.iter().enumerate() {
        gu.shape_text(l, w - 240.0, 35.0 + 16.0 * k as f32, 0x8080_8080, false);
    }
}

/// `select-shot`: the select (or delete) screen offscreen with made-up characters.
pub struct ScreenShot {
    /// Characters in the list (races cycle Hume M .. Galka).
    pub chars: usize,
    /// The cursor moves here after `hold` frames; then `frames` more frames run (30 a second).
    pub cursor: usize,
    pub hold: u32,
    pub frames: u32,
    /// list, options, slots, login, delete, reset, settings, ok, rename.
    pub ui: String,
    pub deleting: bool,
    /// Slots flagged must-rename / cannot-use (name colours).
    pub rename: Option<usize>,
    pub unusable: Option<usize>,
    pub zone: u16,
    pub hour: f32,
    pub width: u32,
    pub height: u32,
}

const SHOT_NAMES: [&str; 16] = ["Lnxtest", "Lnxcsfix", "Aldo", "Bellatrix", "Corvin", "Delphine", "Ewald", "Fiora", "Gaspard", "Hesper", "Ivo", "Juniper", "Kestrel", "Linnea", "Maelis", "Norbert"];

pub fn screen_shot(shot: &ScreenShot, out: &std::path::Path) -> Result<()> {
    use super::game_ui::GameUiData;
    let cfg = crate::config::Config::load();
    let index = Arc::new(DatIndex::open(&cfg.ffxi_dir())?);
    let dll = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?;
    let keys = Arc::new(zone::Keys::from_ffximain(&dll)?);
    let mut gu = GameUi::new(GameUiData::load(&index, &dll)?);
    let n = shot.chars.min(SLOTS);
    let mut chars: Vec<Character> = (0..n)
        .map(|i| Character {
            content_id: 1000 + i as u32,
            name: SHOT_NAMES[i].into(),
            world: "Nameless".into(),
            status: if shot.unusable == Some(i) { 2 } else { 1 },
            rename_required: shot.rename == Some(i),
            race: (i % 8) as u16 + 1,
            main_job: (i % 22) as u8 + 1,
            main_level: 1 + (i as u8 * 7) % 99,
            sub_job: 0,
            face: 0,
            nation: 0,
            size: 1,
            zone: shot.zone,
            looks: [0, 0, 8, 8, 8, 8, 0, 0],
        })
        .collect();
    // Three empty purchased slots after them, as LSB pads the list.
    chars.extend((0..3).map(|_| Character { content_id: 0, name: " ".into(), world: String::new(), status: 1, rename_required: false, race: 0, main_job: 0, main_level: 0, sub_job: 0, face: 0, nation: 0, size: 0, zone: 0, looks: [0; 8] }));
    let list = List::new(&chars);
    let looks: Vec<Option<(Look, u8)>> = (0..SLOTS)
        .map(|i| list.get(&chars, i).map(|c| (Look { race: c.race as u8, face: c.face, head: c.looks[1], body: c.looks[2], hands: c.looks[3], legs: c.looks[4], feet: c.looks[5], main: c.looks[6], sub: c.looks[7], range: 0 }, c.size)))
        .collect();
    let mut data = SelectData::load(&index, &keys, &looks)?;
    data.lighting = ZoneWeather::parse(&index.read(zone::file_id(ZONE))?).ok().and_then(|w| w.at("suny", shot.hour * 60.0)).map(|e| Lighting::from_env(&e, shot.hour * 60.0));
    for (i, s) in data.slots.iter().enumerate().take(n) {
        println!("slot {i}: feet {:.3} point7 {:?}", s.feet, s.point.map(|p| (p.x, p.y, p.z)));
    }
    let mut scene = SelectScene::new(data);
    let mut st = SelectUi::default();
    let (w, h) = (shot.width, shot.height);
    let gpu = Gpu::headless()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target_tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("select shot"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = target_tex.create_view(&Default::default());
    let depth = crate::render::depth_view(&gpu.device, w, h);
    // Every lobby frame is drawn, so the effects run as they would on screen.
    let frame = |scene: &mut SelectScene, cursor: usize| {
        scene.advance(FRAME, cursor);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        scene.render(&gpu, format, &mut enc, (&view, Some(&target_tex), [w, h]), &depth, w as f32 / h.max(1) as f32, None);
        gpu.queue.submit([enc.finish()]);
        let _ = gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        if let Some(fx) = scene.effects() {
            fx.after_submit(&gpu);
        }
    };
    for _ in 0..shot.hold.max(30) {
        frame(&mut scene, st.cursor);
    }
    if shot.cursor != st.cursor {
        st.cursor = shot.cursor.min(SLOTS - 1);
        scene.cursor_moved(st.cursor);
    }
    for _ in 0..shot.frames {
        frame(&mut scene, st.cursor);
    }
    let (eye, target) = scene.camera();
    println!("camera: eye {eye:.3} target {target:.3}");
    st.focus = match shot.ui.as_str() {
        "options" => Focus::Options(1),
        "slots" => Focus::Slots(1, 1),
        "login" => Focus::Prompt(Prompt::Login, true),
        "delete" => Focus::Prompt(Prompt::Delete, false),
        "reset" => Focus::Prompt(Prompt::Reset, false),
        "settings" => Focus::Prompt(Prompt::Settings { save: true, slot: 0 }, false),
        "ok" => Focus::Ok(gu.error_text(3000 + 0x74), Box::new(Focus::List)),
        "rename" => Focus::Rename("Newname".into()),
        _ => Focus::List,
    };
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    scene.advance(FRAME, st.cursor);
    scene.render(&gpu, format, &mut enc, (&view, Some(&target_tex), [w, h]), &depth, w as f32 / h.max(1) as f32, None);
    let input = Input { events: &[], pointer: None, moved: false, clicked: false, busy: false, modern: false };
    st.step(&mut gu, &input, &list, &chars, shot.deleting, w as f32, h as f32, None);
    gu.render(&gpu, format, &mut enc, &view, [w, h], [w, h], true, 1.0, 1.0);
    let row = (w * 4).div_ceil(256) * 256;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    enc.copy_texture_to_buffer(
        target_tex.as_image_copy(),
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
    fn glide_is_constant_speed() {
        // A straight curve: equal steps of L / 75 over the modelled arc L / 2, so 38 frames.
        let mut c = Curve::new(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0));
        let mut last = c.at();
        let mut frames = 0;
        while c.t < 1.0 {
            c.step();
            let p = c.at();
            if c.t < 1.0 {
                assert!(((p - last).length() - 4.0 / 75.0).abs() < 1e-3, "step {} at {frames}", (p - last).length());
            }
            last = p;
            frames += 1;
        }
        assert_eq!(frames, 38);
        assert!((c.at() - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-5);
        // Ends at the start and end points, through neither control point.
        let b = Curve::new(Vec3::ZERO, Vec3::new(1.0, -1.0, 0.0), Vec3::new(2.0, 0.0, 0.0));
        assert!(b.at().length() < 1e-6);
    }

    #[test]
    fn list_compacts_and_counts() {
        let ch = |name: &str, status| Character { content_id: 0, name: name.into(), world: String::new(), status, rename_required: false, race: 1, main_job: 1, main_level: 1, sub_job: 0, face: 0, nation: 0, size: 0, zone: 0, looks: [0; 8] };
        let chars = [ch("Aa", 1), ch(" ", 1), ch("Bb", 2), ch("Cc", 1), ch(" ", 1), ch("Dd", 0)];
        let l = List::new(&chars);
        assert_eq!(l.entries, vec![0, 2, 3]);
        assert_eq!((l.purchased, l.registered), (4, 2));
        assert_eq!(l.get(&chars, 1).map(|c| c.name.as_str()), Some("Bb"));
        assert!(l.get(&chars, 3).is_none());
    }

    #[test]
    fn printf_formats() {
        assert_eq!(printf("No. of characters registered: %2d", &["3"]), "No. of characters registered:  3");
        assert_eq!(printf("Error code: FFXI-%04d", &["3116"]), "Error code: FFXI-3116");
        assert_eq!(printf("Log in with \"%s\"?", &["Lnxtest"]), "Log in with \"Lnxtest\"?");
        assert_eq!(printf("slot %c?", &["A"]), "slot A?");
    }

    #[test]
    fn point7_is_the_chest() {
        let Ok(index) = DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let look = |race| Look { race, face: 0, head: 0, body: 8, hands: 8, legs: 8, feet: 8, main: 0, sub: 0, range: 0 };
        let (Ok(hume), Ok(galka)) = (load_character(&index, &look(1)), load_character(&index, &look(8))) else { return };
        // Rest point 7 above the feet: Hume M 1.322 x size, Galka 1.701 (skeleton DATs 7072 / 26352).
        for (m, y) in [(&hume, -1.322f32), (&galka, -1.701)] {
            let s = m.scale(1);
            let p = rest_point(m, POINT, placement(Vec3::new(4.0, 0.0, 0.0), s)).unwrap();
            assert!((p.y - y * s).abs() < 0.01, "{p} vs {}", y * s);
        }
    }
}
