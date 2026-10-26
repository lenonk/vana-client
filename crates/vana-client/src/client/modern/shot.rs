//! `modern-shot`: the modern layer offscreen without a login: the HUD panels with sample values and every window frame
//! at several sizes, over a screenshot when one is given.

use super::hud::{self, PartyRow, StatusView, TargetView};
use super::{Layout, Modern, Panels, paint};
use crate::render::Gpu;
use anyhow::{Context as _, Result};
use egui::{Color32, Id, LayerId, Order, Pos2, Rect, pos2, vec2};
use std::path::Path;

fn load_png(path: &Path) -> Result<([usize; 2], Vec<u8>)> {
    let mut d = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).with_context(|| path.display().to_string())?));
    d.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut r = d.read_info()?;
    let mut buf = vec![0u8; r.output_buffer_size().unwrap_or(0)];
    let info = r.next_frame(&mut buf)?;
    buf.truncate(info.buffer_size());
    anyhow::ensure!(info.color_type == png::ColorType::Rgba, "{}: not RGBA after expansion", path.display());
    Ok(([info.width as usize, info.height as usize], buf))
}

fn sample(m: &Modern, ctx: &egui::Context, layout: &mut Layout) {
    let mut moved = false;
    let mut p = Panels { m, ctx, layout, moved: &mut moved };
    hud::status_panel(&mut p, &StatusView { name: "Lenon".into(), jobs: "RDM 75 / WHM 37".into(), icon: hud::job_icon(5), hp: (1324, 1345, 0.98), mp: Some((711, 892, 0.8)), tp: 1630 });
    let row = |slot, name: &str, job, here, hp, marked| PartyRow { slot, name: name.into(), icon: hud::job_icon(job), here, hp, marked };
    hud::party_panel(&mut p, &[row(1, "Raven", 1, true, 0.88, false), row(2, "Koru", 3, true, 0.62, true), row(3, "Shantotto", 4, true, 0.95, false), row(4, "Prishe", 2, false, 1.0, false)]);
    hud::target_panel(&mut p, &TargetView { name: "Robber Crab".into(), ring: "indicators/target_hostile", hp: Some((72, 0.72)) });
    // Every frame at a few sizes: nine-slice corners, tiled edges, the stretched gradient.
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::unique("frames")));
    let screen = ctx.content_rect();
    let names = ["window_normal", "window_focused", "window_inactive", "window_alert", "window_tooltip"];
    let sizes = [vec2(90.0, 70.0), vec2(170.0, 110.0), vec2(260.0, 150.0)];
    let mut x = 16.0;
    for (i, n) in names.iter().enumerate() {
        let Some(f) = m.art.frame(n) else { continue };
        let s = sizes[i % sizes.len()];
        painter.add(paint::window(f, Rect::from_min_size(pos2(x, screen.max.y - s.y - 16.0), s), 1.0));
        painter.text(pos2(x + 12.0, screen.max.y - s.y - 2.0), egui::Align2::LEFT_TOP, *n, egui::FontId::proportional(12.0), m.theme.color("muted"));
        x += s.x + 12.0;
    }
}

pub fn shot(width: u32, height: u32, backdrop: Option<&Path>, out: &Path) -> Result<()> {
    let gpu = Gpu::headless()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("modern shot"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let ctx = egui::Context::default();
    let m = Modern::new(&ctx);
    let zoom = m.theme.zoom(height as f32);
    ctx.set_zoom_factor(zoom);
    let bg = backdrop.map(load_png).transpose()?.map(|(size, rgba)| ctx.load_texture("backdrop", egui::ColorImage::from_rgba_unmultiplied(size, &rgba), egui::TextureOptions::LINEAR));
    let mut layout = Layout::new();
    let mut deltas = Vec::new();
    let mut last = None;
    // Frame 0 only sets the screen (egui's first frame has its default one); the time steps finish the fades.
    for frame in 0..8 {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width as f32, height as f32) / zoom)), time: Some(frame as f64 * 0.5), ..Default::default() };
        let output = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx();
            if frame == 0 {
                return;
            }
            if let Some(t) = &bg {
                ctx.layer_painter(LayerId::background()).image(t.id(), ctx.content_rect(), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            sample(&m, ctx, &mut layout);
        });
        for (id, ds) in &output.textures_delta.set {
            deltas.push((*id, ds.clone()));
        }
        last = Some(output);
    }
    let output = last.context("no egui output")?;
    let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
    let mut renderer = egui_wgpu::Renderer::new(&gpu.device, format, Default::default());
    for (id, ds) in &deltas {
        for d in ds {
            renderer.update_texture(&gpu.device, &gpu.queue, *id, d);
        }
    }
    let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [width, height], pixels_per_point: output.pixels_per_point };
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    let extra = renderer.update_buffers(&gpu.device, &gpu.queue, &mut enc, &jobs, &screen);
    {
        let mut pass = enc
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("modern shot"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.04, g: 0.07, b: 0.09, a: 1.0 }), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();
        renderer.render(&mut pass, &jobs, &screen);
    }
    let row = (width * 4).div_ceil(256) * 256;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * height) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    enc.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(height) } },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    gpu.queue.submit(extra.into_iter().chain([enc.finish()]));
    super::super::save_capture(&gpu, &buf, [width, height], row, format, out)?;
    println!("wrote {}", out.display());
    Ok(())
}
