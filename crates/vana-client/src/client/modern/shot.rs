//! `modern-shot`: the modern layer offscreen without a login: the HUD panels with sample values and every window frame
//! at several sizes, over a screenshot when one is given.

use super::chat::{self, ChatView, InputView};
use super::hud::{self, Effect, Icon, PartyRow, StatusView, TargetView, WhereView};
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
    let top = crate::client::hudtop::TopView {
        title: "Status".into(),
        help: "View status.".into(),
        alpha: 1.0,
        shown: true,
        title_up: true,
        net_up: true,
        busy: None,
        pct: 100,
        sent: 154,
        recv: 276,
        meter: (1, 3),
    };
    super::topbar::draw(m, ctx, &top);
    let (mut changed, mut chat_font) = (false, 15.0);
    let mut p = Panels { m, ctx, gu: None, layout, changed: &mut changed, chat_font: &mut chat_font, this: Default::default() };
    // The chat window first, as the layer draws it: the location panel sits on it.
    let wh = [0x80, 0x80, 0x80, 0x80];
    let lines = vec![
        (String::new(), "=== Area: West Ronfaure ===", wh, false),
        ("[18:42:10] ".to_string(), "Raven : rdy?", [0x80, 0x80, 0x80, 0x80], true),
        (String::new(), "Lenon starts casting Protect on Koru.", [0x30, 0x78, 0x80, 0x80], false),
        (String::new(), "Koru gains the effect of Protect.", wh, false),
        (String::new(), "The Robber Crab hits Raven for 24 points of damage.", [0x60, 0x60, 0x80, 0x80], false),
        ("[18:42:31] ".to_string(), "(Koru) \u{E008}Let's go!\u{E009}", [0x80, 0x70, 0x40, 0x80], true),
    ];
    let shown = chat::Shown { rows: 8.0, draw_n: 8, pending: 0, slide: 0 };
    chat::draw(&mut p, &ChatView { lines: lines.iter().map(|(a, b, c, d)| (a.clone(), *b, *c, *d)).collect(), input: Some(InputView { channel: "Party", pieces: vec![("Pulling now ".into(), false), ("Thank you.".into(), true)], caret: 22 }), waiting: false }, shown, true);
    let fx = |icon, label: Option<&str>, ending| Effect { icon: Icon::Pack(icon), label: label.map(str::to_string), ending };
    let mine = vec![fx("icons/protect", Some("25m"), false), fx("icons/haste", Some("2m"), false), fx("icons/refresh", Some("12"), true), fx("icons/poison", None, false)];
    hud::status_panel(&mut p, &StatusView { effects: mine }, false);
    let dot = |x, y, c: u32| (x, y, super::chat::colour(c.to_le_bytes()));
    let compass = hud::CompassView { heading: 0.6, dots: vec![dot(4.0, -9.0, 0x8030_60e0), dot(-11.0, 3.0, 0x8000_b040), dot(14.0, 13.0, 0x5000_b040)], range: 28.8, arrow: Some((0.6, -0.8, "focus")) };
    hud::where_panel(&mut p, &WhereView { zone: "West Ronfaure".into(), grid: "(F-9)".into(), time: (18, 42), day: (3, "Windsday".into()), weather: Some((2, false)), compass: Some(compass) });
    let theirs = vec![fx("icons/protect", None, false), fx("icons/sleep", None, false)];
    let vit = |hp: u32, mp: u32, f: f32| hud::RowVitals { hp, hp_max: Some(1000), mp, tp: Some(if f > 0.9 { 1250 } else { 300 }), hpf: f, mpf: Some(f * 0.8), hp_colour: if f < 0.75 { egui::Color32::from_rgb(0xff, 0xd0, 0x40) } else { egui::Color32::WHITE } };
    let row = |slot, name: &str, job, here: bool, hp: f32, marked, effects: Vec<Effect>| PartyRow { slot, name: name.into(), icon: hud::job_icon(job), here: here.then(|| vit((hp * 1000.0) as u32, 420, hp)), zone: "Port Jeuno".into(), marked, hand: marked.then_some(0x8080_8080), marks: (slot == 0) as u8, effects };
    hud::party_panel(&mut p, "party", "Party", &[row(0, "Lenon", 5, true, 0.98, false, vec![]), row(1, "Raven", 1, true, 0.88, false, theirs.clone()), row(2, "Koru", 3, true, 0.62, true, vec![]), row(3, "Shantotto", 4, true, 0.95, false, vec![]), row(4, "Prishe", 2, false, 1.0, false, vec![])], false);
    hud::cast_panel(&mut p, 0.6);
    hud::target_panel(&mut p, &TargetView { name: "Robber Crab".into(), ring: "indicators/target_hostile", locked: true, hp: Some((72, 0.72)) });
    *m.rects.borrow_mut() = std::mem::take(&mut p.this);
    // Every frame at a few sizes: nine-slice corners, tiled edges, the stretched gradient.
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::unique("frames")));
    let screen = ctx.content_rect();
    let names = ["window_normal", "window_focused", "window_inactive", "window_alert", "window_tooltip"];
    let sizes = [vec2(90.0, 70.0), vec2(170.0, 110.0), vec2(260.0, 150.0)];
    let mut x = screen.center().x - 300.0;
    for (i, n) in names.iter().enumerate() {
        let Some(f) = m.art.frame(n) else { continue };
        let s = sizes[i % sizes.len()];
        let y = screen.center().y - s.y;
        painter.add(paint::window(f, Rect::from_min_size(pos2(x, y), s), 1.0));
        painter.text(pos2(x + 12.0, y + 14.0), egui::Align2::LEFT_TOP, *n, egui::FontId::proportional(12.0), m.theme.color("muted"));
        x += s.x + 12.0;
    }
}

pub fn shot(width: u32, height: u32, scale: u32, backdrop: Option<&Path>, out: &Path) -> Result<()> {
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
    let zoom = m.theme.zoom(height as f32, scale);
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
