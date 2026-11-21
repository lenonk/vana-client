//! Lua HUD portraits (hud.portrait): an actor drawn alone each frame from in front of its face into a small target,
//! which the classic UI shows as an image and the modern UI as a texture. Ours, not retail.

use super::game_ui::GameUi;
use super::lua::hud::Wanted;
use super::{Place, World};
use crate::render::{ActorGpu, Camera, Gpu, ZoneRenderer};
use glam::Vec3;
use std::collections::HashMap;

/// How many portraits draw at most.
pub const SLOTS: usize = 4;

struct Slot {
    entity: u32,
    /// Its target's pixels: twice the size shown, so the shown picture is smoothed.
    size: [u32; 2],
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    depth: wgpu::TextureView,
    egui: egui::TextureId,
    /// Bound as the game UI's image yet.
    in_ui: bool,
}

#[derive(Default)]
pub struct Portraits {
    slots: Vec<Slot>,
    finish: Option<crate::render::portrait::PortraitFinish>,
}

/// The game UI's image name of slot `k`.
fn key(k: usize) -> String {
    format!("portrait {k}")
}

/// A target's pixels for a portrait shown `size` window pixels: twice that, in steps of 16.
fn target_size(size: [f32; 2]) -> [u32; 2] {
    size.map(|v| (((v * 2.0).ceil() as u32).div_ceil(16) * 16).clamp(32, 1024))
}

/// A gamma colour premultiplied, as the target holds it.
fn premul(c: [u8; 4]) -> [f32; 4] {
    let a = c[3] as f32 / 255.0;
    [c[0] as f32 / 255.0 * a, c[1] as f32 / 255.0 * a, c[2] as f32 / 255.0 * a, a]
}

/// The camera on an actor's face, level and from straight in front: the top of the model to the shoulders in view,
/// a little more round it in an oval. Its head point (3, the one others look at) sits about at the neck.
pub(super) fn camera(w: &World, places: &HashMap<u32, Place>, id: u32, oval: bool) -> Option<Camera> {
    let p = places.get(&id)?;
    let a = w.actors.get(&id)?;
    let head = super::attach_point(w, places, id, 3)?;
    // y is down: the model's top is above the head point.
    let top = p.pos.y + a.top;
    let span = (head.y - top).abs().max(0.05);
    let centre = Vec3::new(head.x, top + span * if oval { 0.55 } else { 0.5 }, head.z);
    let fov_y = 30f32.to_radians();
    let dist = span * if oval { 0.72 } else { 0.66 } / (fov_y / 2.0).tan();
    let f = Vec3::new(p.forward.x, 0.0, p.forward.z).normalize_or(Vec3::X);
    Some(Camera { jitter: glam::Vec2::ZERO, eye: centre + f * dist, target: centre, up: Vec3::NEG_Y, fov_y })
}

impl Portraits {
    /// Draws each wanted actor into a slot, a slot kept while its actor is wanted, and finishes it (backing, oval,
    /// frame). The ones drawn: entity -> (game UI image, modern UI texture).
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        egui: &mut egui_wgpu::Renderer,
        mut gu: Option<&mut GameUi>,
        r: &mut ZoneRenderer,
        encoder: &mut wgpu::CommandEncoder,
        format: wgpu::TextureFormat,
        want: &[(Wanted, Camera, &ActorGpu)],
    ) -> HashMap<u32, (String, egui::TextureId)> {
        let mut out = HashMap::new();
        let wanted: Vec<u32> = want.iter().map(|w| w.0.entity).collect();
        for &(p, cam, a) in want.iter().take(SLOTS) {
            let size = target_size(p.size);
            let k = match self.slots.iter().position(|s| s.entity == p.entity) {
                Some(k) => k,
                None => match self.slots.iter().position(|s| !wanted.contains(&s.entity)) {
                    Some(k) => {
                        self.slots[k].entity = p.entity;
                        k
                    }
                    None => {
                        self.slots.push(Slot::new(gpu, egui, format, p.entity, size));
                        self.slots.len() - 1
                    }
                },
            };
            if self.slots[k].size != size {
                egui.free_texture(&self.slots[k].egui);
                self.slots[k] = Slot::new(gpu, egui, format, p.entity, size);
            }
            let s = &mut self.slots[k];
            if !s.in_ui
                && let Some(g) = gu.as_deref_mut()
            {
                g.view(&key(k), s.tex.create_view(&Default::default()));
                s.in_ui = true;
            }
            let aspect = size[0] as f32 / size[1] as f32;
            if !r.render_portrait(
                gpu,
                encoder,
                k,
                &s.view,
                &s.depth,
                &cam,
                aspect,
                p.look.bg.map_or([0.0; 4], premul),
                a,
            ) {
                continue;
            }
            let f = crate::render::portrait::Finish {
                oval: p.look.oval,
                frame: p.look.frame.map(premul),
                frame_px: p.look.frame_w * size[1] as f32 / p.size[1].max(1.0),
            };
            self.finish
                .get_or_insert_with(|| crate::render::portrait::PortraitFinish::new(gpu, format))
                .run(gpu, encoder, k, &s.view, size, f);
            if s.in_ui {
                out.insert(p.entity, (key(k), s.egui));
            }
        }
        out
    }
}

impl Slot {
    fn new(gpu: &Gpu, egui: &mut egui_wgpu::Renderer, format: wgpu::TextureFormat, entity: u32, px: [u32; 2]) -> Self {
        let size = wgpu::Extent3d { width: px[0], height: px[1], depth_or_array_layers: 1 };
        let make = |label, format, usage| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color =
            make("portrait", format, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let depth = make("portrait depth", crate::render::DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let view = color.create_view(&Default::default());
        // The target is gamma-encoded as the scene is, as egui's textures are.
        let id = egui.register_native_texture(&gpu.device, &view, wgpu::FilterMode::Linear);
        Self {
            entity,
            size: px,
            tex: color,
            view,
            depth: depth.create_view(&Default::default()),
            egui: id,
            in_ui: false,
        }
    }
}
