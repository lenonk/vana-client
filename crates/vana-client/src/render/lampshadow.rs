//! Lamp shadows (ours, not retail): with Real character shadows and Real lamp lights (client/lamps.rs), the few lamps
//! nearest the camera that have characters near them each get a small perspective depth map of the characters, seen
//! from the lamp. The zone shader's per-pixel point lights (zone.wgsl point_lights) look a lamp's slot up here and
//! scale that lamp's light by the map's visibility, so a character cuts only the lamp's own light out of the scenery
//! behind it: the baked lighting and the other lights are untouched, and characters themselves are not shaded by it.

use super::Gpu;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

/// Lamps shadowed at once (the map's layers).
pub const LAYERS: usize = 4;
/// Each layer's size in texels.
pub const SIZE: u32 = 512;
/// A character this close to a lamp casts from it (client/lamps.rs REACH, the floor lamp's reach).
pub const REACH: f32 = 16.0;
/// How much of the lamp's light a full shadow takes away.
pub const STRENGTH: f32 = 0.85;
/// A character as a sphere: its centre this far above the feet (FFXI's y points down), this radius.
const CENTRE: f32 = 0.9;
const RADIUS: f32 = 1.2;
const NEAR: f32 = 0.25;
/// The widest half-angle a lamp's map covers; characters outside it cast nothing from that lamp.
const MAX_HALF: f32 = 60.0 * std::f32::consts::PI / 180.0;
const MIN_HALF: f32 = 5.0 * std::f32::consts::PI / 180.0;

/// The zone shader's view (group 0 binding 5): per layer the lamp's light slot + 1 (0 = unused) and its matrix;
/// misc = (strength, PCF step in uv, 0, 0).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct LampShadowU {
    pub slots: [u32; 4],
    pub vp: [[[f32; 4]; 4]; LAYERS],
    pub misc: [f32; 4],
}

/// The light matrix of a lamp at `lamp` for the characters at `feet` (positions), aimed at their centre and widened
/// to take them all in (up to MAX_HALF). None when none is within REACH.
pub fn matrix(lamp: Vec3, feet: &[Vec3]) -> Option<Mat4> {
    let centres: Vec<Vec3> = feet.iter().map(|p| *p - Vec3::Y * CENTRE).filter(|c| c.distance(lamp) < REACH).collect();
    if centres.is_empty() {
        return None;
    }
    let mid = centres.iter().copied().sum::<Vec3>() / centres.len() as f32;
    // Casters around the lamp itself (a low lamp over a head) give no direction: look straight down (y is down).
    let dir = (mid - lamp).try_normalize().filter(|_| mid.distance(lamp) > RADIUS).unwrap_or(Vec3::Y);
    let half = centres
        .iter()
        .map(|c| {
            let d = c.distance(lamp);
            if d <= RADIUS {
                return MAX_HALF;
            }
            dir.dot((*c - lamp) / d).clamp(-1.0, 1.0).acos() + (RADIUS / d).min(1.0).asin()
        })
        .fold(0.0f32, f32::max)
        .clamp(MIN_HALF, MAX_HALF);
    let up = if dir.y.abs() > 0.9 { Vec3::X } else { Vec3::Y };
    Some(glam::camera::rh::proj::directx::perspective(2.0 * half, 1.0, NEAR, REACH + RADIUS) * glam::camera::rh::view::look_at_mat4(lamp, lamp + dir, up))
}

/// This frame's shadowed lamps from `lamps` (position, light slot): those with a character of `feet` in reach, the
/// nearest to `target` (the camera's) first, at most LAYERS.
pub fn pick(lamps: &[(Vec3, u32)], feet: &[Vec3], target: Vec3) -> Vec<(u32, Mat4)> {
    let mut near: Vec<(f32, Vec3, u32)> = lamps.iter().filter(|(p, _)| feet.iter().any(|f| f.distance(*p) < REACH)).map(|&(p, s)| (p.distance(target), p, s)).collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    near.into_iter().filter_map(|(_, p, s)| Some((s, matrix(p, feet)?))).take(LAYERS).collect()
}

/// The maps, their matrices for the depth passes (group 3 of the silhouette layout, 256 bytes apart) and the zone
/// shader's uniform, comparison sampler and array view (bound in the zone's globals).
pub struct LampShadows {
    pub layers: Vec<wgpu::TextureView>,
    pub array: wgpu::TextureView,
    pub cmp: wgpu::Sampler,
    pub uniform: wgpu::Buffer,
    pub matrices: wgpu::Buffer,
    pub matrix_group: wgpu::BindGroup,
}

impl LampShadows {
    pub fn new(gpu: &Gpu, sil_layout: &wgpu::BindGroupLayout) -> Self {
        let device = &gpu.device;
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lamp shadow maps"),
            size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: LAYERS as u32 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: super::DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let layers = (0..LAYERS as u32)
            .map(|l| tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), base_array_layer: l, array_layer_count: Some(1), ..Default::default() }))
            .collect();
        let array = tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let cmp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lamp shadow compare"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor { label: Some("lamp shadows"), size: std::mem::size_of::<LampShadowU>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        gpu.queue.write_buffer(&uniform, 0, bytemuck::bytes_of(&LampShadowU::zeroed()));
        let matrices = device.create_buffer(&wgpu::BufferDescriptor { label: Some("lamp shadow matrices"), size: 256 * LAYERS as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let matrix_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lamp shadow matrices"),
            layout: sil_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &matrices, offset: 0, size: wgpu::BufferSize::new(64) }) }],
        });
        Self { layers, array, cmp, uniform, matrices, matrix_group }
    }

    /// Writes this frame's lamps (light slot, matrix) for the depth passes and the zone shader; none clears them.
    pub fn set(&self, gpu: &Gpu, lamps: &[(u32, Mat4)]) {
        let mut u = LampShadowU::zeroed();
        for (k, (slot, m)) in lamps.iter().take(LAYERS).enumerate() {
            u.slots[k] = slot + 1;
            u.vp[k] = m.to_cols_array_2d();
            super::Put::put(&gpu.queue, &self.matrices, 256 * k as u64, bytemuck::cast_slice(&m.to_cols_array()));
        }
        u.misc = [STRENGTH, 1.0 / SIZE as f32, 0.0, 0.0];
        super::Put::put(&gpu.queue, &self.uniform, 0, bytemuck::bytes_of(&u));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(m: Mat4, p: Vec3) -> Vec3 {
        let c = m * p.extend(1.0);
        c.truncate() / c.w
    }

    /// The map takes in every character near the lamp, in front of it and inside the depth range.
    #[test]
    fn matrix_frames_the_characters() {
        let lamp = Vec3::new(0.0, -4.0, 0.0);
        let feet = [Vec3::new(3.0, 0.0, 1.0), Vec3::new(-2.0, 0.0, 4.0), Vec3::new(1.0, 0.0, -3.0)];
        let m = matrix(lamp, &feet).unwrap();
        for f in feet {
            let p = project(m, f - Vec3::Y * CENTRE);
            assert!(p.x.abs() < 1.0 && p.y.abs() < 1.0 && (0.0..1.0).contains(&p.z), "{f} -> {p}");
        }
        // A low lamp right over a character still casts, looking down at it.
        let m = matrix(Vec3::new(0.0, -1.8, 0.0), &[Vec3::ZERO]).unwrap();
        let p = project(m, Vec3::ZERO);
        assert!(p.x.abs() < 1.0 && p.y.abs() < 1.0 && (0.0..1.0).contains(&p.z), "feet -> {p}");
        // Too far: none.
        assert!(matrix(lamp, &[Vec3::new(40.0, 0.0, 0.0)]).is_none());
    }

    /// Only lamps with a character in reach, nearest the camera first, at most LAYERS.
    #[test]
    fn pick_nearest_lamps_with_casters() {
        let feet = [Vec3::ZERO];
        let lamps: Vec<(Vec3, u32)> = (0..8).map(|i| (Vec3::new(i as f32 * 3.0, -3.0, 0.0), 10 + i)).chain([(Vec3::new(100.0, -3.0, 0.0), 99)]).collect();
        let got = pick(&lamps, &feet, Vec3::new(6.0, 0.0, 0.0));
        let slots: Vec<u32> = got.iter().map(|g| g.0).collect();
        assert_eq!(slots.len(), LAYERS);
        assert_eq!(slots[0], 12);
        assert!(!slots.contains(&99));
    }
}
