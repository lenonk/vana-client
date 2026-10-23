//! flash-shot: a Hume in a zone at an hour, GPU-skinned as the client draws actors, with the target flash at
//! k = 0, 0.5 and 1 (client/flash.rs), each to a PNG.

use crate::{config, dat, render};
use anyhow::Result;
use glam::{Mat4, Vec3};

pub fn run(zone: u32, hour: f32, pos: [f32; 3], out_dir: &std::path::Path) -> Result<()> {
    let cfg = config::Config::load();
    let index = dat::DatIndex::open(&cfg.ffxi_dir())?;
    let keys = dat::zone::Keys::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?)?;
    let file = index.read(crate::dat::zone::file_id(zone))?;
    let scene = dat::zone::load_scene(&keys, &file)?;
    let weather = dat::weather::ZoneWeather::parse(&file)?;
    let lighting = weather.at("suny", hour * 60.0).map(|e| dat::weather::Lighting::from_env(&e, hour * 60.0));
    let ch = std::sync::Arc::new(dat::character::load_character(&index, &dat::character::Look { race: 1, face: 0, head: 0, body: 8, hands: 8, legs: 8, feet: 8, main: 0, sub: 0 })?);
    let gpu = render::Gpu::headless()?;
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut r = render::ZoneRenderer::new(&gpu, &scene, format);
    r.lighting = lighting;
    let p = Vec3::from(pos);
    let (w, h) = (800u32, 600u32);
    let camera = render::Camera { eye: p + Vec3::new(2.2, -1.6, 2.2), target: p + Vec3::new(0.0, -0.9, 0.0), up: Vec3::NEG_Y, fov_y: 40f32.to_radians() };
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let depth = render::depth_view(&gpu.device, w, h);
    let mut actor = r.new_skinned(&gpu, &ch);
    let idle: Vec<_> = ch.find_animations("idl").into_iter().map(|a| (a, 0.0)).collect();
    let model = Mat4::from_translation(p) * Mat4::from_rotation_y(-std::f32::consts::FRAC_PI_4);
    for (k, name) in [(0.0f32, "flash_k0.png"), (0.5, "flash_k05.png"), (1.0, "flash_k1.png")] {
        actor.flash = [k, k];
        r.begin_poses();
        r.pose_actor_gpu(&mut actor, &ch, &[], &idle, &[], 1.0, model, None, None);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        r.upload_poses(&gpu, &mut enc);
        r.render(&gpu, &mut enc, &view, &depth, &camera, w as f32 / h as f32, &[&actor]);
        let row = (w * 4).div_ceil(256) * 256;
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        enc.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None })?;
        let data = buf.slice(..).get_mapped_range().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let px: Vec<u8> = (0..h).flat_map(|y| data[(y * row) as usize..(y * row + w * 4) as usize].to_vec()).collect();
        drop(data);
        let path = out_dir.join(name);
        render::ui::write_png(&path, w, h, &px)?;
        println!("wrote {} (flash k {k})", path.display());
    }
    Ok(())
}
