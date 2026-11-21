//! The title screen's 3D backdrop: one random field movie (mov1..mov8 of file 23's "loop", which
//! "main" spawns) played behind the lobby art, with its camera paths, fades and crossfades.

use crate::dat::title::{Op, TITLE_FILE, Title, fov_y, hour_of, sample};
use crate::dat::weather::{Env, Lighting, ZoneWeather};
use crate::dat::{DatIndex, zone};
use crate::render::ui::{UiBatch, UiRenderer};
use crate::render::{Camera, Gpu, ZoneRenderer};
use anyhow::{Context, Result};
use std::sync::Arc;

/// Texture name of the offscreen frame a crossfade fades out.
const PREVIOUS: &str = "\0flyover previous";

/// Parsed movie: its ops, camera paths and zone; loaded off the UI thread.
pub struct FlyoverData {
    title: Title,
    ops: Vec<Op>,
    scene: zone::ZoneScene,
    /// The zone's collision, for the camera list under the movie camera.
    ground: super::ground::Ground,
    /// The zone's weather keys, and the title DAT's 0x2f blocks that op 0x7e applies.
    weather: Option<ZoneWeather>,
    envs: Vec<(String, Env)>,
    /// The rest of this pass of "loop", played next.
    pub rest: Vec<String>,
    /// The movie's zone (op 0x7b).
    zone: u32,
    /// The zone's effect libraries: op 0x7b loads the whole scene (FUN_1018b3a0), so its auto-start generators run,
    /// their sounds too (birds, water, crowds).
    fx: Option<super::select::FxLibs>,
}

impl FlyoverData {
    /// The next movie of "loop": `queue`'s first, or a new pass's (`enabled`: the content bits op 0xaf tests).
    pub fn load_next(index: &Arc<DatIndex>, keys: &Arc<zone::Keys>, queue: Vec<String>, enabled: u32) -> Result<Self> {
        let mut q = queue;
        if q.is_empty() {
            let title = Title::parse(&index.read(TITLE_FILE)?)?;
            let mut seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64 | 1);
            q = title.playlist(enabled, |n| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                (seed % n.max(1) as u64) as u32
            });
        }
        anyhow::ensure!(!q.is_empty(), "loop plays no movie");
        let movie = q.remove(0);
        let mut d = Self::load_file(index, keys, TITLE_FILE, &movie)?;
        d.rest = q;
        Ok(d)
    }

    /// A movie of any file laid out like 23 (the creator's nation files 24..26, FUN_1023ad60); the zone comes
    /// from the movie's op 0x7b or else from "main"'s.
    pub fn load_file(index: &Arc<DatIndex>, keys: &Arc<zone::Keys>, file_id: u32, movie: &str) -> Result<Self> {
        let file = index.read(file_id)?;
        let title = Title::parse(&file)?;
        let envs = crate::dat::chunks(&file)?
            .into_iter()
            .filter(|c| c.kind == 0x2f)
            .filter_map(|c| Env::parse(c.data).map(|e| (c.name(), e)))
            .collect();
        let ops = title.scheduler(movie)?.to_vec();
        let zone_of = |ops: &[Op]| ops.iter().find_map(|o| if let Op::Zone(z) = o { Some(*z) } else { None });
        let zone = zone_of(&ops)
            .or_else(|| title.scheduler("main").ok().and_then(zone_of))
            .with_context(|| format!("{movie} loads no zone"))?;
        let zone_file = index.read(crate::dat::zone::file_id(zone))?;
        let scene = zone::load_scene(keys, &zone_file)?;
        let ground = super::ground::Ground::build(&scene);
        let weather = ZoneWeather::parse(&zone_file).ok();
        let fx = super::select::FxLibs::load_zone(index, keys, zone, &zone_file);
        log::info!("flyover {file_id} {movie}: zone {zone}, {} ops", ops.len());
        Ok(Self { title, ops, scene, ground, weather, envs, rest: Vec::new(), zone, fx })
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
    /// The crossfade's outgoing frame, and whether it holds this crossfade's frame yet.
    previous: Option<wgpu::Texture>,
    captured: bool,
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
    /// Op 0x0e's blur (r, g, b, a, zoom: from, to, start tick, length), its restarts, and its pass.
    blur: ([f32; 5], [f32; 5], f32, f32),
    blur_starts: u32,
    screen_fx: Option<crate::render::screenfx::ScreenFx>,
    /// The zone's effects, with their sounds.
    effects: Option<super::select::LobbyFx>,
    /// The Clipping setting's factor (config 0x3e) for the fog, as in the world.
    pub clip_x: f32,
    /// Settings' Draw Distance (100 Retail, 0 Unlimited): past Retail the camera list holds within retail's reach.
    pub draw_distance: u32,
    pub done: bool,
}

impl Flyover {
    /// `voices`: the effects' voice handle base (select::TITLE_VOICES, NATION_VOICES).
    pub fn new(mut data: FlyoverData, voices: u32) -> Self {
        let minute = 12.5 * 60.0;
        let effects = data.fx.take().map(|l| super::select::LobbyFx::new(l, minute, voices));
        Self {
            data,
            zone: None,
            ui: None,
            previous: None,
            captured: false,
            tick: 0.0,
            next_at: 0.0,
            cursor: 0,
            shot: None,
            fade: ([0.0; 3], [0.0; 3], 0.0, 0.0),
            cross: None,
            minute,
            weather: "suny".into(),
            env: None,
            blur: ([0.0; 5], [0.0; 5], 0.0, 0.0),
            blur_starts: 0,
            screen_fx: None,
            effects,
            clip_x: 1.0,
            draw_distance: 0,
            done: false,
        }
    }

    /// Plays the movie again from the start (the nation files' "loop" repeats mov1).
    pub fn restart(&mut self) {
        (self.tick, self.next_at, self.cursor, self.shot, self.cross, self.done) = (0.0, 0.0, 0, None, None, false);
        self.captured = false;
    }

    /// Starts at full brightness: the nation movies have no fade op (the creator's 'ffin' fades them in).
    pub fn unfaded(mut self) -> Self {
        self.fade = ([1.0; 3], [1.0; 3], 0.0, 0.0);
        self
    }

    /// Runs the ops due by now (each op waits its `wait` ticks before the next one starts).
    pub fn advance(&mut self, dt: f32) {
        self.tick += dt * 60.0;
        if let Some(fx) = &mut self.effects {
            fx.advance(dt);
        }
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
                // Case 0x0e (as the world's, client/event.rs fx_op): from the blur drawn now.
                Op::Blur { rgba, zoom, wait, ticks } => {
                    let f = |v: u8| v as f32 / 255.0;
                    let to = [f(rgba[0]), f(rgba[1]), f(rgba[2]), f(rgba[3]), zoom];
                    self.blur = (self.blur_now(), to, now, ticks as f32);
                    self.blur_starts += 1;
                    self.next_at = now + wait as f32;
                }
                Op::Crossfade { wait, ticks } => {
                    if let Some(cam) = self.camera() {
                        self.cross = Some((cam, now, ticks.max(1) as f32));
                        self.captured = false;
                    }
                    self.next_at = now + wait as f32;
                }
                // The lobby clock starts at 12:30 (FUN_1023b3f0); 0x7d sets it, 0x7e applies a title
                // 0x2f block, 0x7c switches to the zone's keys for a weather.
                Op::Time(t) => {
                    self.minute = hour_of(t) * 60.0;
                    if let Some(fx) = &mut self.effects {
                        fx.set_minute(self.minute);
                    }
                }
                Op::Env(name) => self.env = self.data.envs.iter().find(|e| e.0 == name).map(|e| e.1),
                Op::Weather { tag, .. } => {
                    if let Some(fx) = &mut self.effects {
                        fx.set_weather(&tag);
                    }
                    self.weather = tag;
                    self.env = None;
                }
                Op::Other(0) if self.cursor > 1 => self.done = true,
                _ => {}
            }
        }
    }

    fn blur_now(&self) -> [f32; 5] {
        let (from, to, start, len) = self.blur;
        let t = if len <= 0.0 { 1.0 } else { ((self.tick - start) / len).clamp(0.0, 1.0) };
        std::array::from_fn(|i| from[i] + (to[i] - from[i]) * t)
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
        // A path with no keys has no camera (as in event.rs's camera); sampling it would index an empty list.
        if path.keys.is_empty() {
            return None;
        }
        let k = sample(path, (self.tick - s.start) / s.ticks);
        Some(Camera {
            jitter: glam::Vec2::ZERO,
            eye: k.eye,
            target: k.target,
            up: crate::dat::title::up_of(k.eye, k.target, k.roll),
            fov_y: fov_y(k.focal),
        })
    }

    /// Effect sounds since the last call, for the sound thread.
    pub fn take_sounds(&mut self) -> Vec<crate::fx::sound::SoundCmd> {
        self.effects.as_mut().map(|fx| fx.take_sounds()).unwrap_or_default()
    }

    /// The movie is not drawn this frame: the stops for its voices (loops start again when it is).
    pub fn pause_sounds(&mut self) -> Vec<crate::fx::sound::SoundCmd> {
        self.effects.as_mut().map(|fx| fx.pause_sounds()).unwrap_or_default()
    }

    /// The movie is going: the stops for every voice it still holds.
    pub fn stop_sounds(&mut self) -> Vec<crate::fx::sound::SoundCmd> {
        self.effects.as_mut().map(|fx| fx.stop_sounds()).unwrap_or_default()
    }

    /// After the frame's submit: the effects' readback.
    pub fn after_submit(&mut self, gpu: &Gpu) {
        if let Some(fx) = &mut self.effects {
            fx.after_submit(gpu);
        }
    }

    /// Draws the zone and its effects through the movie camera into `view`, with any crossfade and fade. `alive`:
    /// the sound thread's live voices. Returns false (nothing drawn) before the first camera op.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        encoder: &mut wgpu::CommandEncoder,
        (view, tex): (&wgpu::TextureView, Option<&wgpu::Texture>),
        depth: &wgpu::TextureView,
        size: [u32; 2],
        aspect: f32,
        alive: Option<Vec<u32>>,
    ) -> bool {
        let Some(cam) = self.camera() else { return false };
        let cross = self.cross.filter(|(_, start, len)| self.tick < start + len);
        let f = self.fade_colour();
        let b = self.blur_now();
        let tick = self.tick;
        let env = self.env.or_else(|| self.data.weather.as_ref().and_then(|w| w.at(&self.weather, self.minute)));
        // Op 0x7b (FUN_1018b3a0, as the 0x00A zone-in): the zone's fog and draw constants under the Clipping factor.
        let zone_id = self.data.zone as u16;
        let lighting = env.map(|e| {
            let k = crate::dat::weather::clipping(e.draw_dist, self.clip_x, zone_id).1;
            Lighting::from_env_k(&e, self.minute, k)
        });
        let zone = self.zone.get_or_insert_with(|| ZoneRenderer::new(gpu, &self.data.scene, format));
        zone.lighting = lighting;
        // The lobby state (FUN_10013ad0) probes under its camera each frame (FUN_101829c0) and puts that camera list
        // in force (FUN_1017a960), as the world does: without it, a zone that places a detailed mesh and a simpler
        // stand-in at one spot (Ru'Aun Gardens' ground and gates) draws both, and they fight for depth.
        // Past Retail, as the world: the list holds within retail's distance and the rest draws beyond it.
        let ground = &self.data.ground;
        let reach = crate::dat::weather::clipping(env.map_or(100.0, |e| e.draw_dist), self.clip_x, zone_id).0;
        let far = (self.draw_distance != 100).then_some(reach).filter(|d| *d > 0.0);
        let ui = self.ui.get_or_insert_with(|| UiRenderer::new(gpu, format));
        let mut batch = UiBatch::default();
        if let Some((old, start, len)) = cross {
            // Op 0x10, as the world's crossfade (event.rs Cross): the frame before the cut, drawn once offscreen
            // from the camera it had, then faded out over the new shot. Its own submit keeps that camera in the
            // zone's uniforms: drawn in this frame's encoder, both passes would read the new camera's while the old
            // one picked the runs drawn, and whole runs of scenery would drop out of it.
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
                self.captured = false;
            }
            if !self.captured {
                zone.set_camera_list(gpu, ground.camera_list(old.eye), far);
                let prev_view = self.previous.as_ref().unwrap().create_view(&Default::default());
                let mut enc = gpu.device.create_command_encoder(&Default::default());
                zone.render(gpu, &mut enc, &prev_view, depth, &old, aspect, &[]);
                gpu.queue.submit([enc.finish()]);
                self.captured = true;
            }
            let a = 1.0 - ((tick - start) / len).clamp(0.0, 1.0);
            batch.push_image(PREVIOUS, 0.0, 0.0, size[0] as f32, size[1] as f32, [1.0, 1.0, 1.0, a]);
        }
        zone.set_camera_list(gpu, ground.camera_list(cam.eye), far);
        zone.render(gpu, encoder, view, depth, &cam, aspect, &[]);
        if let Some(fx) = &mut self.effects {
            fx.draw(gpu, format, encoder, (view, tex, size), depth, zone, &cam, aspect, alive);
        }
        // FUN_100132b0's second block: last frame over the scene while the blur's alpha is not 0.
        let blur = (b[3] * 255.0 >= 0.5).then(|| crate::render::screenfx::Blur {
            rgb: [b[0], b[1], b[2]],
            a: (2.0 * b[3]).min(1.0),
            zoom: b[4],
            start: self.blur_starts,
        });
        if blur.is_some() || self.screen_fx.is_some() {
            let fx = self.screen_fx.get_or_insert_with(|| crate::render::screenfx::ScreenFx::new(gpu, format));
            fx.draw_all(gpu, encoder, view, tex, size, None, None, blur, [1.0; 3]);
        }
        // Fade: a multiply of the scene colour, drawn as black over it.
        let dim = (1.0 - (f[0] + f[1] + f[2]) / 3.0).clamp(0.0, 1.0);
        if dim > 0.0 {
            batch.push_rect(0.0, 0.0, size[0] as f32, size[1] as f32, [0.0, 0.0, 0.0, dim]);
        }
        ui.render(gpu, encoder, view, size, size, None, &batch);
        true
    }
}
