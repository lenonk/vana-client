//! `creator-shot`: one frame of the character creator preview (dancer engine) rendered offscreen.

use crate::dat::{DatIndex, dancer};
use crate::render::character::{PosedMesh, ShotOptions, render_batches_png};
use crate::render::dancer as rd;
use anyhow::{Context, Result, ensure};
use glam::{Mat3, Mat4, Quat, Vec3};

pub struct Shot {
    /// Creator race index 0..7 (HumeM HumeF ElvM ElvF TaruM TaruF Mithra Galka).
    pub race: usize,
    pub face: usize,
    /// 0 = hair A (CHG1 materials), 1 = hair B (CHG2).
    pub hair: usize,
    /// Performance frame (30 per second).
    pub frame: f32,
    /// 0/1 = recorded camera 0/1 (view modes 0/1), 2 = idle loop with a fixed front camera (mode 2 approximation).
    pub view: u8,
    pub width: u32,
    pub height: u32,
    /// Leave the background transparent (the dancer clears only depth; the lobby scene shows through).
    pub transparent: bool,
    /// Draw over this PNG (same size as the output), e.g. the zone-133 backdrop.
    pub backdrop: Option<std::path::PathBuf>,
    /// Old single-light textured preview (for comparisons).
    pub flat: bool,
}

/// CHG material swap (FUN_10266130): shapes on `xxxCHG1MT` use `xxxCHG{hair+1}MT`.
pub fn apply_hair(m: &mut dancer::Model, hair: usize) {
    let want = (b'1' + hair as u8) as char;
    for s in &mut m.shapes {
        let Some(name) = m.materials.get(s.shader) else { continue };
        if let Some(at) = name.find("CHG") {
            let mut alt = name.clone();
            alt.replace_range(at + 3..at + 4, &want.to_string());
            if let Some(i) = m.materials.iter().position(|n| *n == alt) {
                s.shader = i;
            }
        }
    }
}

fn read_png(path: &std::path::Path, w: u32, h: u32) -> Result<Vec<u8>> {
    let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path)?));
    let mut r = dec.read_info()?;
    let mut buf = vec![0; r.output_buffer_size().context("png size")?];
    let info = r.next_frame(&mut buf)?;
    ensure!(info.width == w && info.height == h, "backdrop is {}x{}, output {w}x{h}", info.width, info.height);
    let px = &buf[..info.buffer_size()];
    Ok(match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        t => anyhow::bail!("backdrop colour type {t:?}"),
    })
}

pub fn render(index: &DatIndex, shot: &Shot, out: &std::path::Path) -> Result<()> {
    use dancer::creator::{LIGHTS, MODEL_GROUP, MOTION_GROUP};
    ensure!(shot.race < 8 && shot.face < 8 && shot.hair < 2, "race 0..7, face 0..7, hair 0..1");
    let (mg, mo) = (MODEL_GROUP[shot.race], MOTION_GROUP[shot.race]);
    let load = |id: u32, tag: &str| -> Result<dancer::Model> {
        dancer::load_model(&index.read(id)?, &index.read(id + 1)?, tag).with_context(|| format!("dancer model {id}"))
    };
    let chan = |id: u32| -> Result<dancer::Channels> { dancer::parse_channels(&index.read(id)?).with_context(|| format!("channels {id}")) };
    let body = load(mg + 2, "b:")?;
    let mut head = load(mg + 4 + 2 * shot.face as u32, "h:")?;
    apply_hair(&mut head, shot.hair);
    let idle = shot.view == 2;
    let (bm, hm) = if idle { (mo + 9, mo + 0xf + 6 * shot.face as u32) } else { (mo + 8, mo + 0xe + 6 * shot.face as u32) };
    let (bc, hc) = (chan(bm)?, chan(hm)?);
    let fr = |c: &dancer::Channels| if idle { shot.frame % c.frames as f32 } else { shot.frame.min(c.frames as f32 - 1.0) };
    let (bf, hf) = (bc.sample(fr(&bc)), hc.sample(fr(&hc)));
    let mut mesh = PosedMesh { batches: Vec::new(), min: Vec3::splat(f32::MAX), max: Vec3::splat(f32::MIN) };
    dancer::skin_model(&body, Some(&bf), &mut mesh);
    dancer::skin_model(&head, Some(&hf), &mut mesh);

    let aspect = shot.width as f32 / shot.height as f32;
    // Camera-to-world transform and projection.
    let (cam, proj) = if shot.view < 2 {
        // Camera k: FOV channel (degrees, inferred vertical for the 4:3 dancer target) + camera-to-world matrix, -z forward.
        let k = shot.view as u32;
        let (fc, cc) = (chan(mo + 2 * k)?, chan(mo + 2 * k + 1)?);
        let f = shot.frame.min(cc.frames as f32 - 1.0);
        let fov = fc.sample(f)[0];
        let m = cc.sample(f);
        let cam = Mat4::from_cols_array(&[m[0], m[4], m[8], m[12], m[1], m[5], m[9], m[13], m[2], m[6], m[10], m[14], m[3], m[7], m[11], m[15]]);
        println!("camera {k}: pos {} fov {fov:.2}", cam.w_axis.truncate());
        (cam, glam::camera::rh::proj::directx::perspective(fov.to_radians(), aspect, 0.5, 4000.0))
    } else {
        let c = (mesh.min + mesh.max) / 2.0;
        let eye = c + Vec3::new(0.0, 0.0, (mesh.max.y - mesh.min.y) * 2.4);
        let view = glam::camera::rh::view::look_at_mat4(eye, c, Vec3::Y);
        (view.inverse(), glam::camera::rh::proj::directx::perspective(30f32.to_radians(), aspect, 0.5, 4000.0))
    };
    let view_proj = proj * cam.inverse();
    if shot.flat {
        let mut textures = Vec::new();
        for t in body.textures.iter().chain(&head.textures) {
            let mut o = t.clone();
            o.name = format!("{}#opaque", t.name);
            o.rgba.chunks_exact_mut(4).for_each(|p| p[3] = 255);
            textures.push(t.clone());
            textures.push(o);
        }
        let opts = ShotOptions { cull: false, clear: [0.02, 0.025, 0.05, if shot.transparent { 0.0 } else { 1.0 }], light: [0.3, 0.6, 0.75, 0.45] };
        render_batches_png(&textures, &mesh, view_proj, &opts, shot.width, shot.height, out)?;
    } else {
        // Lights ride on the camera: node = camera q x light q (FUN_10266820; the camera is identity at attach, FUN_102637f0).
        let rcam = Mat3::from_mat4(cam);
        let lights: Vec<rd::Light> = LIGHTS
            .iter()
            .map(|(q, c)| {
                let frame = rcam * Mat3::from_quat(Quat::from_array(*q).normalize());
                rd::Light { dir: frame.z_axis.normalize(), color: *c, frame }
            })
            .collect();
        let (eye, fwd) = (cam.w_axis.truncate(), -cam.z_axis.truncate().normalize());
        let mut draws = Vec::new();
        rd::build_draws(&body, &dancer::skin_shapes(&body, Some(&bf)), eye, fwd, &lights, &mut draws);
        rd::build_draws(&head, &dancer::skin_shapes(&head, Some(&hf)), eye, fwd, &lights, &mut draws);
        let textures: Vec<_> = body.textures.iter().chain(&head.textures).cloned().collect();
        let bg = match (&shot.backdrop, shot.transparent) {
            (Some(p), _) => rd::Background::Image(read_png(p, shot.width, shot.height)?),
            (None, true) => rd::Background::Transparent,
            (None, false) => rd::Background::Color([0.02, 0.025, 0.05]),
        };
        rd::render_png(&textures, &draws, view_proj, &bg, shot.width, shot.height, out)?;
        println!("{} draws", draws.len());
    }
    println!("bounds {} .. {}; wrote {}", mesh.min, mesh.max, out.display());
    Ok(())
}
