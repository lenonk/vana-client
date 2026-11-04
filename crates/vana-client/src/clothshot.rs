//! cloth-shot: a model file with a cloth mesh (whole, or a PC part on its race), its cloth headers and the skeleton's
//! spheres printed, then simulated (client/cloth.rs) standing in the wind and walking, each to a PNG.

use crate::client::cloth::{Cloth, Frame, Wind};
use crate::{config, dat, render};
use anyhow::{Context, Result};
use glam::{Mat4, Vec3};

/// The (race, slot, model id) whose part file is `file`.
fn look_of(file: u32) -> Option<(u8, usize, u16)> {
    (0..=8u8)
        .flat_map(|r| (1..6).map(move |s| (r, s)))
        .find_map(|(r, s)| (0..0x1000u16).find(|&id| dat::character::part_file(r, s, id) == Some(file)).map(|id| (r, s, id)))
}

pub fn run(file: u32, zone: u32, hour: f32, pos: [f32; 3], steps: u32, out_dir: &std::path::Path) -> Result<()> {
    let cfg = config::Config::load();
    let index = dat::DatIndex::open(&cfg.ffxi_dir())?;
    // Retail's cloth meshes are in whole-model files (a flag, NPCs), not PC gear; a PC part file loads on its race.
    let ch = std::sync::Arc::new(match look_of(file) {
        Some((race, slot, id)) => {
            let mut look = dat::character::Look { race, face: 0, head: 0, body: 0, hands: 0, legs: 0, feet: 0, main: 0, sub: 0, range: 0 };
            *[&mut look.face, &mut look.head, &mut look.body, &mut look.hands, &mut look.legs, &mut look.feet][slot] = id;
            println!("file {file}: race {race} {} {id}", dat::character::SLOTS[slot]);
            dat::character::load_character(&index, &look)?
        }
        None => {
            println!("file {file}: a whole model");
            dat::npc::load_file_model(&index, file)?
        }
    });
    let sk = &ch.skeleton;
    println!("skeleton {}: {} bones, {} sphere slots (used ones below)", sk.name, sk.bones.len(), sk.spheres.len());
    for (i, s) in sk.spheres.iter().enumerate() {
        if s.bone >= 0 && (s.bone as usize) < sk.bones.len() && s.radius > 0.0 {
            println!("  sphere {i}: bone {} radius {:.4} offset {:.4?}", s.bone, s.radius, s.offset.to_array());
        }
    }
    for p in ch.parts.iter().filter(|p| p.cloth) {
        match &p.sim {
            Some(s) => println!(
                "cloth {}: {}+{} particles, pinned {}+{}, {} edges ({} group 1), k {:?}, damping {}, gravity {}, inertia {}, spheres {:?}, {} triangles",
                p.name.trim_end_matches(['\0', ' ']),
                s.counts[0],
                s.counts[1],
                s.pins[0],
                s.pins[1],
                s.edges.len(),
                s.group1,
                s.k,
                s.damping,
                s.gravity,
                s.inertia,
                s.spheres,
                s.tris.len()
            ),
            None => println!("cloth {} (mirrored {}): no simulation", p.name.trim_end_matches(['\0', ' ']), p.mirrored),
        }
    }
    let mut cloth = Cloth::new(&ch.parts).context("no simulated cloth on this model")?;
    let file = index.read(dat::zone::file_id(zone))?;
    let keys = dat::zone::Keys::from_ffximain(&std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll"))?)?;
    let scene = dat::zone::load_scene(&keys, &file)?;
    let weather = dat::weather::ZoneWeather::parse(&file)?;
    let gpu = render::Gpu::headless()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut r = render::ZoneRenderer::new(&gpu, &scene, format);
    r.lighting = weather.at("suny", hour * 60.0).map(|e| dat::weather::Lighting::from_env(&e, hour * 60.0));
    let mut actor = r.new_skinned(&gpu, &ch);
    let idle: Vec<_> = ch.find_animations("idl").into_iter().map(|a| (a, 0.0)).collect();
    let bones = dat::character::pose(&ch.skeleton, &idle);
    let top = render::character::skinned_top(&ch, &bones).min(-0.5);
    let p0 = Vec3::from(pos);
    // Side on to the wind (world -x): the model's +x (its front) turned to +z, the way it walks.
    let at = |p: Vec3| Mat4::from_translation(p) * Mat4::from_rotation_y(-std::f32::consts::FRAC_PI_2);
    let mut wind = Wind::default();
    let mut model = at(p0);
    let mut shots: Vec<(String, Mat4, Vec<[f32; 4]>)> = vec![("cloth_rest.png".into(), model, Vec::new())];
    let pack = |c: &Cloth, m: Mat4| {
        let inv = m.inverse();
        c.sheets
            .iter()
            .flat_map(|s| s.p.iter())
            .flat_map(|q| [inv.transform_point3(q.cur).extend(1.0).to_array(), inv.transform_vector3(q.normal).normalize_or_zero().extend(1.0).to_array()])
            .collect::<Vec<_>>()
    };
    let rest: Vec<Vec3> = {
        let mut c = Cloth::new(&ch.parts).unwrap();
        c.step(&ch.parts, &Frame { skeleton: sk, bones: &bones, model, moved: Vec3::ZERO, wind: None });
        c.sheets.iter().flat_map(|s| s.p.iter().map(|q| q.cur)).collect()
    };
    let report = |label: &str, c: &Cloth, rest: &[Vec3], off: Vec3| {
        let d: Vec<f32> = c.sheets.iter().flat_map(|s| s.p.iter()).zip(rest).map(|(q, r)| (q.cur - (*r + off)).length()).collect();
        let max = d.iter().copied().fold(0.0, f32::max);
        let bad = c.sheets.iter().flat_map(|s| s.p.iter()).any(|q| !q.cur.is_finite());
        println!("{label}: free particles moved up to {max:.4} from the skin{}", if bad { " (NOT FINITE)" } else { "" });
    };
    // Standing: the gusts at their stock size, then 0x97-sized ones (base 40, spread 20).
    for (name, base, spread) in [("cloth_wind.png", 4, 10), ("cloth_gale.png", 40, 20)] {
        wind.set(base, spread);
        for _ in 0..steps {
            wind.step(2.0);
            cloth.step(&ch.parts, &Frame { skeleton: sk, bones: &bones, model, moved: Vec3::ZERO, wind: Some((wind.dir, wind.strength)) });
        }
        report(name, &cloth, &rest, Vec3::ZERO);
        shots.push((name.into(), model, pack(&cloth, model)));
    }
    wind.restore();
    // Walking along +z at 0.1 a step (3 a second).
    let mut p = p0;
    for _ in 0..steps {
        p.z += 0.1;
        let m = at(p);
        let moved = m.w_axis.truncate() - model.w_axis.truncate();
        model = m;
        wind.step(2.0);
        cloth.step(&ch.parts, &Frame { skeleton: sk, bones: &bones, model, moved, wind: Some((wind.dir, wind.strength)) });
    }
    report("cloth_walk.png", &cloth, &rest, p - p0);
    shots.push(("cloth_walk.png".into(), model, pack(&cloth, model)));
    let (w, h) = (800u32, 600u32);
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
    for (name, m, data) in shots {
        let feet = m.w_axis.truncate();
        // Framed on the model's height (y down: the top is negative).
        let look = feet + Vec3::new(0.0, top * 0.5, 0.0);
        let camera =
            render::Camera { eye: look + Vec3::new(2.6, -0.5, -1.2) * (top.abs() / 1.8).max(1.0), target: look, up: Vec3::NEG_Y, fov_y: 40f32.to_radians() };
        actor.cloth = data;
        r.begin_poses();
        r.pose_actor_gpu(&mut actor, &ch, &[], &idle, &[], 1.0, m, None, None, &[]);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        r.upload_poses(&gpu, &mut enc);
        r.render(&gpu, &mut enc, &view, &depth, &camera, w as f32 / h as f32, &[&actor]);
        let row = (w * 4).div_ceil(256) * 256;
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
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
        let path = out_dir.join(&name);
        render::ui::write_png(&path, w, h, &px)?;
        println!("wrote {}", path.display());
    }
    Ok(())
}
