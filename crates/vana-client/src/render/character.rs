//! Offscreen renderer for one posed character (CPU skinning).

use super::{Camera, Gpu, depth_view};
use crate::dat::character::{BoneXf, Character, pose, pose_held, skin};
use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use std::collections::HashMap;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CharVertex {
    pos: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    light: [f32; 4],
    misc: [f32; 4],
}

/// Skinned triangles grouped by texture name.
pub struct PosedMesh {
    /// Texture, (light factor, cloth 0 / 1), posed corners.
    pub batches: Vec<(String, [f32; 2], Vec<[f32; 8]>)>,
    pub min: Vec3,
    pub max: Vec3,
}

/// Poses the skeleton with the given animation frames and skins every part.
pub fn pose_character(ch: &Character, anims: &[(&crate::dat::character::Animation, f32)]) -> PosedMesh {
    skin_character(ch, &pose(&ch.skeleton, anims))
}

/// `pose_character` with drawn weapons on their grips.
pub fn pose_character_held(
    ch: &Character,
    anims: &[(&crate::dat::character::Animation, f32)],
    grips: &[(u16, u16)],
) -> PosedMesh {
    skin_character(ch, &pose_held(&ch.skeleton, anims, grips))
}

/// Highest point (smallest y, y down) of the skinned mesh, positions only.
pub fn skinned_top(ch: &Character, world: &[BoneXf]) -> f32 {
    let mut top = f32::MAX;
    for part in &ch.parts {
        for inf in &part.vertices {
            let mut y = 0.0;
            for i in inf {
                let b = world.get(i.bone).copied().unwrap_or(BoneXf::IDENTITY);
                y += (b.q * (b.s * i.pos)).y + b.t.y * i.weight;
            }
            top = top.min(y);
        }
    }
    top
}

fn skin_character(ch: &Character, world: &[BoneXf]) -> PosedMesh {
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let mut batches: Vec<(String, [f32; 2], Vec<[f32; 8]>)> = Vec::new();
    for part in &ch.parts {
        let skinned = skin(part, world);
        for g in &part.groups {
            let mut out = Vec::with_capacity(g.triangles.len() * 3);
            for t in &g.triangles {
                for c in t {
                    let (p, n) = skinned[c.vertex as usize];
                    min = min.min(p);
                    max = max.max(p);
                    out.push([p.x, p.y, p.z, n.x, n.y, n.z, c.uv[0], c.uv[1]]);
                }
            }
            batches.push((g.texture.clone(), [g.light, part.cloth as u32 as f32], out));
        }
    }
    PosedMesh { batches, min, max }
}

pub fn render_png(
    ch: &Character,
    mesh: &PosedMesh,
    camera: &Camera,
    width: u32,
    height: u32,
    path: &std::path::Path,
) -> Result<()> {
    let view_proj = camera.view_proj(width as f32 / height as f32);
    let opts = ShotOptions { cull: true, clear: [0.25, 0.3, 0.38, 1.0], light: [0.4, -0.8, -0.5, 0.45] };
    render_batches_png(&ch.textures, mesh, view_proj, &opts, width, height, path)
}

/// Render settings for render_batches_png.
pub struct ShotOptions {
    pub cull: bool,
    /// RGBA; alpha 0 leaves the background transparent in the PNG.
    pub clear: [f64; 4],
    /// xyz towards the light, w ambient.
    pub light: [f32; 4],
}

/// Draws texture-keyed triangle batches with an explicit view-projection matrix.
pub fn render_batches_png(
    tex_list: &[crate::dat::zone::Texture],
    mesh: &PosedMesh,
    view_proj: glam::Mat4,
    opts: &ShotOptions,
    width: u32,
    height: u32,
    path: &std::path::Path,
) -> Result<()> {
    let gpu = Gpu::headless()?;
    let device = &gpu.device;
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("character"),
        source: wgpu::ShaderSource::Wgsl(include_str!("character.wgsl").into()),
    });
    let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("globals"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("texture"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("character"),
        bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
        immediate_size: 0,
    });
    let vertex_layouts = [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<CharVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
    })];
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("character"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &vertex_layouts,
        },
        // File triangles (and the swapped mirrored copies) are clockwise from the front.
        primitive: wgpu::PrimitiveState {
            front_face: wgpu::FrontFace::Cw,
            cull_mode: opts.cull.then_some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: super::DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(format.into())],
        }),
        multiview_mask: None,
        cache: None,
    });

    let u = Uniforms { view_proj: view_proj.to_cols_array_2d(), light: opts.light, misc: [0.0; 4] };
    let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("uniforms"),
        contents: bytemuck::bytes_of(&u),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let globals = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("globals"),
        layout: &globals_layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let make_texture = |w: u32, h: u32, rgba: &[u8]| {
        let texture = device.create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            Default::default(),
            rgba,
        );
        let view = texture.create_view(&Default::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &texture_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        })
    };
    // Slot 0: magenta for missing textures.
    let mut textures = vec![make_texture(1, 1, &[255, 0, 255, 255])];
    let mut slot: HashMap<&str, usize> = HashMap::new();
    for t in tex_list {
        slot.insert(&t.name, textures.len());
        textures.push(make_texture(t.width, t.height, t.rgba()));
    }
    let mut verts: Vec<CharVertex> = Vec::new();
    let mut draws = Vec::new();
    for (tex, _, tris) in &mesh.batches {
        let start = verts.len() as u32;
        verts.extend(tris.iter().map(|v| CharVertex {
            pos: [v[0], v[1], v[2]],
            normal: [v[3], v[4], v[5]],
            uv: [v[6], v[7]],
        }));
        draws.push((slot.get(tex.as_str()).copied().unwrap_or(0), start..verts.len() as u32));
    }
    if verts.is_empty() {
        anyhow::bail!("nothing to draw");
    }
    let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("char"),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let depth = depth_view(device, width, height);
    let row = (width * 4).div_ceil(256) * 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("character"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: opts.clear[0],
                        g: opts.clear[1],
                        b: opts.clear[2],
                        a: opts.clear[3],
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &globals, &[]);
        pass.set_vertex_buffer(0, vbuf.slice(..));
        for (tex, range) in &draws {
            pass.set_bind_group(1, &textures[*tex], &[]);
            pass.draw(range.clone(), 0..1);
        }
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(height) },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    gpu.queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None })?;
    let data = readback.slice(..).get_mapped_range().map_err(|e| anyhow::anyhow!("mapping readback: {e:?}"))?;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        pixels.extend_from_slice(&data[(y * row) as usize..(y * row + width * 4) as usize]);
    }
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path)?), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header()?.write_image_data(&pixels)?;
    Ok(())
}

#[cfg(test)]
mod queue_shots {
    use super::*;
    use crate::dat::character::{Animation, Look, PoseLayer, load_character, pose_layers};
    use crate::motionq::{Enq, Queues};

    /// Frames of a Hume M through the motion queues, into VANA_QUEUE_SHOTS (a folder): a walk turning into a run
    /// (the blend at 0, 8 and 16 ticks) and a spoken line's mouth cut off by FUN_100ce160 (0, 4 and 9 ticks after).
    /// Run by hand: `VANA_QUEUE_SHOTS=dir cargo test queue_frames -- --ignored`.
    #[test]
    #[ignore]
    fn queue_frames() {
        let Ok(dir) = std::env::var("VANA_QUEUE_SHOTS") else { return };
        let index = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()).unwrap();
        let look = Look { race: 1, face: 0, head: 0, body: 8, hands: 8, legs: 8, feet: 8, main: 0, sub: 0, range: 0 };
        let ch = load_character(&index, &look).unwrap();
        let chunks = |n: &str| -> Vec<(u8, &Animation, f32, f32)> {
            ch.find_animations(n)
                .into_iter()
                .map(|a| (a.name.as_bytes()[3] - b'0', a, a.frames as f32, a.speed))
                .collect()
        };
        let shot = |q: &Queues<&Animation>, name: &str, yaw: f32| {
            let mut l = Vec::new();
            let over = q.layers(&mut l);
            let pl: Vec<PoseLayer> = l
                .iter()
                .map(|x| PoseLayer { anim: x.anim, frame: x.frame, weight: x.weight, start: x.start, over: x.over })
                .collect();
            let mesh = skin_character(&ch, &pose_layers(&ch.skeleton, &pl, over, &[]));
            let centre = (mesh.min + mesh.max) / 2.0;
            let size = (mesh.max - mesh.min).max_element();
            let (s, c) = yaw.to_radians().sin_cos();
            let eye = centre + glam::Vec3::new(s, 0.0, c) * size * 1.6;
            let camera = Camera {
                jitter: glam::Vec2::ZERO,
                eye,
                target: centre,
                up: glam::Vec3::NEG_Y,
                fov_y: 40f32.to_radians(),
            };
            let path = std::path::Path::new(&dir).join(format!("{name}.png"));
            render_png(&ch, &mesh, &camera, 450, 600, &path).unwrap();
            println!(
                "{name}: {}",
                l.iter()
                    .map(|x| format!("{}@{:.1} w{:.2}", x.anim.name, x.frame, x.weight))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        };
        let mut q: Queues<&Animation> = Queues::default();
        q.enqueue_motion(chunks("wlk"), 0, &Enq::status(1.0, 16.0, true), |_| 1.0);
        for _ in 0..25 {
            q.tick(1.0);
        }
        q.enqueue_motion(chunks("run"), 0, &Enq::status(1.0, 16.0, true), |_| 1.0);
        for (k, n) in [(0, "run-blend-00"), (8, "run-blend-08"), (8, "run-blend-16")] {
            for _ in 0..k {
                q.tick(1.0);
            }
            shot(&q, n, 60.0);
        }
        let mut q: Queues<&Animation> = Queues::default();
        q.enqueue_motion(chunks("idl"), 0, &Enq::status(1.0, 16.0, true), |_| 1.0);
        let mou = ch.animations.iter().find(|a| a.name == "mou4").or(ch.animations.iter().find(|a| a.name == "mou6"));
        let Some(mou) = mou else {
            println!("no mouth motion in the model");
            return;
        };
        let talk =
            Enq { share: 1.0, rate: 1.0, count: 1, blend: 8.0, out_len: 1.0, start: 0.0, end: 0.0, carry: false };
        q.enqueue(4, mou, mou.frames as f32, mou.speed, &talk);
        for _ in 0..12 {
            q.tick(1.0);
        }
        q.fade_region(4, 8.0);
        for (k, n) in [(0, "mouth-off-0"), (4, "mouth-off-4"), (5, "mouth-off-9")] {
            for _ in 0..k {
                q.tick(1.0);
            }
            shot(&q, n, 0.0);
        }
    }
}
