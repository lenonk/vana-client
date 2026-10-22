//! The title screen's 3D backdrop: one random field movie (mov1..mov8 of file 23's "loop", which
//! "main" spawns) played behind the lobby art, with its camera paths, fades and crossfades.

use crate::dat::title::{Op, Title, TITLE_FILE, fov_y, hour_of, sample};
use crate::dat::weather::{Env, Lighting, ZoneWeather};
use crate::dat::{DatIndex, zone};
use crate::render::ui::{UiBatch, UiRenderer};
use crate::render::{Camera, Gpu, ZoneRenderer};
use anyhow::{Context, Result};
use glam::Vec3;

/// The field movies "loop" picks from at random (op 0x3d).
const MOVIES: [&str; 8] = ["mov1", "mov2", "mov3", "mov4", "mov5", "mov6", "mov7", "mov8"];
/// Texture name of the offscreen frame a crossfade fades out.
const PREVIOUS: &str = "\0flyover previous";

/// Parsed movie: its ops, camera paths and zone; loaded off the UI thread.
pub struct FlyoverData {
    title: Title,
    ops: Vec<Op>,
    scene: zone::ZoneScene,
    /// The zone's weather keys, and the title DAT's 0x2f blocks that op 0x7e applies.
    weather: Option<ZoneWeather>,
    envs: Vec<(String, Env)>,
}

/// A random movie, seeded from the clock.
pub fn pick_movie() -> &'static str {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    MOVIES[n as usize % MOVIES.len()]
}

impl FlyoverData {
    pub fn load(index: &DatIndex, keys: &zone::Keys, movie: &str) -> Result<Self> {
        Self::load_file(index, keys, TITLE_FILE, movie)
    }

    /// A movie of any file laid out like 23 (the creator's nation files 24..26, FUN_1023ad60); the zone comes
    /// from the movie's op 0x7b or else from "main"'s.
    pub fn load_file(index: &DatIndex, keys: &zone::Keys, file_id: u32, movie: &str) -> Result<Self> {
        let file = index.read(file_id)?;
        let title = Title::parse(&file)?;
        let envs = crate::dat::chunks(&file)?.into_iter().filter(|c| c.kind == 0x2f).filter_map(|c| Env::parse(c.data).map(|e| (c.name(), e))).collect();
        let ops = title.scheduler(movie)?.to_vec();
        let zone_of = |ops: &[Op]| ops.iter().find_map(|o| if let Op::Zone(z) = o { Some(*z) } else { None });
        let zone = zone_of(&ops).or_else(|| title.scheduler("main").ok().and_then(zone_of)).with_context(|| format!("{movie} loads no zone"))?;
        let zone_file = index.read(100 + zone)?;
        let scene = zone::load_scene(keys, &zone_file)?;
        let weather = ZoneWeather::parse(&zone_file).ok();
        log::info!("flyover {file_id} {movie}: zone {zone}, {} ops", ops.len());
        Ok(Self { title, ops, scene, weather, envs })
    }
}

#[derive(Clone, Copy)]
struct Shot {
    path: usize,
    start: f32,
    ticks: f32,
}

pub struct Flyover {
    data: FlyoverData,
    zone: Option<ZoneRenderer>,
    ui: Option<UiRenderer>,
    previous: Option<wgpu::Texture>,
    /// Movie clock in ticks (1/60 s), when the next op runs, and the op cursor.
    tick: f32,
    next_at: f32,
    cursor: usize,
    shot: Option<Shot>,
    /// Colour multiply (0x80 = 1.0 per channel): from, to, start tick, length.
    fade: ([f32; 3], [f32; 3], f32, f32),
    /// Crossfade: the outgoing camera, start tick, length.
    cross: Option<(Camera, f32, f32)>,
    /// Lighting state: clock (minutes), weather tag, and an op 0x7e override.
    minute: f32,
    weather: String,
    env: Option<Env>,
    pub done: bool,
}

impl Flyover {
    pub fn new(data: FlyoverData) -> Self {
        Self { data, zone: None, ui: None, previous: None, tick: 0.0, next_at: 0.0, cursor: 0, shot: None, fade: ([0.0; 3], [0.0; 3], 0.0, 0.0), cross: None, minute: 12.5 * 60.0, weather: "suny".into(), env: None, done: false }
    }

    /// Plays the movie again from the start (the nation files' "loop" repeats mov1).
    pub fn restart(&mut self) {
        (self.tick, self.next_at, self.cursor, self.shot, self.cross, self.done) = (0.0, 0.0, 0, None, None, false);
    }

    /// Starts at full brightness: the nation movies have no fade op (the creator's own 'ffin' fades them in).
    pub fn unfaded(mut self) -> Self {
        self.fade = ([1.0; 3], [1.0; 3], 0.0, 0.0);
        self
    }

    /// Runs the ops due by now (each op waits its `wait` ticks before the next one starts).
    pub fn advance(&mut self, dt: f32) {
        self.tick += dt * 60.0;
        while !self.done && self.tick >= self.next_at {
            let Some(op) = self.data.ops.get(self.cursor).cloned() else {
                self.done = true;
                break;
            };
            self.cursor += 1;
            let now = self.next_at;
            match op {
                Op::Camera { path, wait, ticks } => {
                    if let Some(i) = self.data.title.paths.iter().position(|p| p.name == path) {
                        self.shot = Some(Shot { path: i, start: now, ticks: ticks.max(1) as f32 });
                    }
                    self.next_at = now + wait as f32;
                }
                Op::Fade { rgba, wait, ticks } => {
                    let to = [rgba[0], rgba[1], rgba[2]].map(|c| c as f32 / 128.0);
                    self.fade = (self.fade_colour(), to, now, ticks as f32);
                    self.next_at = now + wait as f32;
                }
                Op::Crossfade { wait, ticks } => {
                    if let Some(cam) = self.camera() {
                        self.cross = Some((cam, now, ticks.max(1) as f32));
                    }
                    self.next_at = now + wait as f32;
                }
                // The lobby clock starts at 12:30 (FUN_1023b3f0); 0x7d sets it, 0x7e applies a title
                // 0x2f block, 0x7c switches to the zone's own keys for a weather.
                Op::Time(t) => self.minute = hour_of(t) * 60.0,
                Op::Env(name) => self.env = self.data.envs.iter().find(|e| e.0 == name).map(|e| e.1),
                Op::Weather { tag, .. } => {
                    self.weather = tag;
                    self.env = None;
                }
                Op::Other(0) if self.cursor > 1 => self.done = true,
                _ => {}
            }
        }
    }

    fn fade_colour(&self) -> [f32; 3] {
        let (from, to, start, len) = self.fade;
        let t = if len <= 0.0 { 1.0 } else { ((self.tick - start) / len).clamp(0.0, 1.0) };
        [0, 1, 2].map(|i| from[i] + (to[i] - from[i]) * t)
    }

    /// The current camera on its path.
    pub fn camera(&self) -> Option<Camera> {
        let s = self.shot?;
        let path = &self.data.title.paths[s.path];
        let k = sample(path, (self.tick - s.start) / s.ticks);
        Some(Camera { eye: k.eye, target: k.target, up: Vec3::NEG_Y, fov_y: fov_y(k.focal) })
    }

    /// Draws the zone through the movie camera into `view`, with any crossfade and fade.
    /// Returns false (nothing drawn) before the first camera op.
    pub fn render(&mut self, gpu: &Gpu, format: wgpu::TextureFormat, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, depth: &wgpu::TextureView, size: [u32; 2], aspect: f32) -> bool {
        let Some(cam) = self.camera() else { return false };
        let cross = self.cross.filter(|(_, start, len)| self.tick < start + len);
        let f = self.fade_colour();
        let tick = self.tick;
        let env = self.env.or_else(|| self.data.weather.as_ref().and_then(|w| w.at(&self.weather, self.minute)));
        let lighting = env.map(|e| Lighting::from_env(&e, self.minute));
        let zone = self.zone.get_or_insert_with(|| ZoneRenderer::new(gpu, &self.data.scene, format));
        zone.lighting = lighting;
        let ui = self.ui.get_or_insert_with(|| UiRenderer::new(gpu, format));
        let mut batch = UiBatch::default();
        if let Some((old, start, len)) = cross {
            // The outgoing shot renders offscreen, then fades out over the new one.
            let fits = self.previous.as_ref().is_some_and(|t| t.width() == size[0] && t.height() == size[1]);
            if !fits {
                let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("flyover previous"),
                    size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                ui.insert_view(gpu, PREVIOUS, &tex.create_view(&Default::default()));
                self.previous = Some(tex);
            }
            let prev_view = self.previous.as_ref().unwrap().create_view(&Default::default());
            zone.render(gpu, encoder, &prev_view, depth, &old, aspect, &[]);
            let a = 1.0 - ((tick - start) / len).clamp(0.0, 1.0);
            batch.push_image(PREVIOUS, 0.0, 0.0, size[0] as f32, size[1] as f32, [1.0, 1.0, 1.0, a]);
        }
        zone.render(gpu, encoder, view, depth, &cam, aspect, &[]);
        // Fade: a multiply of the scene colour, drawn as black over it.
        let dim = (1.0 - (f[0] + f[1] + f[2]) / 3.0).clamp(0.0, 1.0);
        if dim > 0.0 {
            batch.push_rect(0.0, 0.0, size[0] as f32, size[1] as f32, [0.0, 0.0, 0.0, dim]);
        }
        ui.render(gpu, encoder, view, size[0], size[1], None, &batch);
        true
    }
}
