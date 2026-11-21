//! Mods' post-process passes (render.shader, ours): a WGSL fragment over the drawn scene, given a copy of the frame so
//! far and its depth, drawn in order over the world before the screen fades and the 2D layer.

use super::Gpu;

/// The full-screen triangle both headers end with.
macro_rules! vertex {
    () => {
        r#"
struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}
"#
    };
}

/// What every pass's source is put after: its bindings and the full-screen triangle. The mod writes
/// `@fragment fn fs_main(in: VsOut) -> @location(0) vec4<f32>`.
pub const HEADER: &str = concat!(
    r#"
struct Post { size: vec2<f32>, time: f32, pad: f32, p: array<vec4<f32>, 4> };
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> post: Post;
@group(0) @binding(3) var depth: texture_depth_2d;"#,
    vertex!()
);

/// The same header on the GL backend, whose GLSL output can't `textureLoad` a `texture_depth_2d`: `depth` is bound as
/// an unfilterable float texture (the same Depth32Float view, the same values in `.x`) and the mod's
/// `textureLoad(depth, ...)` calls are rewritten to take `.x` (`gl_source`), so the header keeps its line count.
const HEADER_GL: &str = concat!(
    r#"
struct Post { size: vec2<f32>, time: f32, pad: f32, p: array<vec4<f32>, 4> };
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> post: Post;
@group(0) @binding(3) var depth: texture_2d<f32>;"#,
    vertex!()
);

/// A mod's source for `HEADER_GL`: every `textureLoad(depth, ...)` becomes `textureLoad(depth, ...).x`, the f32 a
/// `texture_depth_2d` load gives. Read as WGSL tokens, so spaces and comments anywhere in the call don't hide it.
fn gl_source(src: &str) -> String {
    let toks = wgsl_tokens(src);
    let text = |k: usize| toks.get(k).map(|&(a, b)| &src[a..b]);
    let mut at = Vec::new();
    for k in 0..toks.len() {
        if text(k) != Some("textureLoad") || text(k + 1) != Some("(") || text(k + 2) != Some("depth") {
            continue;
        }
        if text(k + 3) != Some(",") {
            continue;
        }
        let mut level = 0i32;
        for j in k + 1..toks.len() {
            match text(j) {
                Some("(") => level += 1,
                Some(")") => {
                    level -= 1;
                    if level == 0 {
                        at.push(toks[j].1);
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    // Found in call order; a load inside another closes first.
    at.sort_unstable();
    let mut out = String::with_capacity(src.len() + 2 * at.len());
    let mut last = 0;
    for p in at {
        out.push_str(&src[last..p]);
        out.push_str(".x");
        last = p;
    }
    out.push_str(&src[last..]);
    out
}

/// WGSL's tokens as byte ranges: identifiers and numbers whole, every other character alone; whitespace, line comments
/// and (nesting) block comments left out.
fn wgsl_tokens(src: &str) -> Vec<(usize, usize)> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i..].starts_with(b"//") {
            i = b[i..].iter().position(|&c| c == b'\n').map_or(b.len(), |n| i + n);
        } else if b[i..].starts_with(b"/*") {
            let mut depth = 0;
            while i < b.len() {
                if b[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if b[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if b[i].is_ascii_whitespace() {
            i += 1;
        } else if b[i].is_ascii_alphanumeric() || b[i] == b'_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push((start, i));
        } else {
            // One character (a multi-byte one whole).
            let n = src[i..].chars().next().map_or(1, char::len_utf8);
            out.push((i, i + n));
            i += n;
        }
    }
    out
}

/// One mod pass as the mods want it this frame: (mod, id, version, source, parameters).
pub type Want = (String, String, u64, std::sync::Arc<str>, [[f32; 4]; 4]);

struct Pass {
    key: (String, String, u64),
    pipeline: Option<wgpu::RenderPipeline>,
    uniform: wgpu::Buffer,
    /// Its group, by the copy and the depth it binds.
    group: super::BindCache<(wgpu::TextureView, wgpu::TextureView)>,
}

pub struct PostFx {
    format: wgpu::TextureFormat,
    /// On the GL backend (`HEADER_GL`).
    gl: bool,
    layout: wgpu::BindGroupLayout,
    pl_layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    copy: Option<(wgpu::Texture, wgpu::TextureView)>,
    passes: Vec<Pass>,
}

impl PostFx {
    /// Passes drawing into views of `format` (the scene's plain view).
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let d = &gpu.device;
        let gl = gpu.adapter.get_info().backend == wgpu::Backend::Gl;
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty,
            count: None,
        };
        let layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mod post"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(1, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(
                    2,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(
                    3,
                    wgpu::BindingType::Texture {
                        sample_type: if gl {
                            wgpu::TextureSampleType::Float { filterable: false }
                        } else {
                            wgpu::TextureSampleType::Depth
                        },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
            ],
        });
        let pl_layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mod post"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let sampler = d.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self { format, gl, layout, pl_layout, sampler, copy: None, passes: Vec::new() }
    }

    /// A pass's pipeline from its source, or the WGSL / validation error.
    fn build(&self, gpu: &Gpu, src: &str) -> Result<wgpu::RenderPipeline, String> {
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mod post"),
            source: wgpu::ShaderSource::Wgsl(
                match self.gl {
                    true => format!("{HEADER_GL}\n{}", gl_source(src)),
                    false => format!("{HEADER}\n{src}"),
                }
                .into(),
            ),
        });
        let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mod post"),
            layout: Some(&self.pl_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        match pollster::block_on(scope.pop()) {
            Some(e) => Err(e.to_string()),
            None => Ok(pipeline),
        }
    }

    /// Keeps the passes the mods want, building new and changed ones: the errors as (mod, id, error).
    pub fn sync(&mut self, gpu: &Gpu, want: &[Want]) -> Vec<(String, String, String)> {
        let mut errors = Vec::new();
        let mut next = Vec::with_capacity(want.len());
        for (m, id, ver, src, _) in want {
            let key = (m.clone(), id.clone(), *ver);
            if let Some(k) = self.passes.iter().position(|p| p.key == key) {
                next.push(self.passes.swap_remove(k));
                continue;
            }
            let pipeline = match self.build(gpu, src) {
                Ok(p) => Some(p),
                Err(e) => {
                    errors.push((m.clone(), id.clone(), e));
                    None
                }
            };
            let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mod post"),
                size: 80,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            next.push(Pass { key, pipeline, uniform, group: Default::default() });
        }
        self.passes = next;
        errors
    }

    /// Each pass over `view` (the scene's plain view of `tex`), reading a copy of what is drawn so far.
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        enc: &mut wgpu::CommandEncoder,
        tex: &wgpu::Texture,
        view: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        time: f32,
        want: &[Want],
    ) {
        let (w, h) = (tex.width(), tex.height());
        if self.copy.as_ref().is_none_or(|(t, _)| t.width() != w || t.height() != h || t.format() != tex.format()) {
            let t = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("mod post copy"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: tex.format(),
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[self.format],
            });
            let v = t.create_view(&wgpu::TextureViewDescriptor { format: Some(self.format), ..Default::default() });
            self.copy = Some((t, v));
        }
        let Some((copy, copy_view)) = &self.copy else { return };
        for (pass, (.., params)) in self.passes.iter().zip(want) {
            let Some(pipeline) = &pass.pipeline else { continue };
            let mut u = vec![w as f32, h as f32, time, 0.0];
            u.extend(params.iter().flatten());
            super::Put::put(&gpu.queue, &pass.uniform, 0, bytemuck::cast_slice(&u));
            enc.copy_texture_to_texture(
                tex.as_image_copy(),
                copy.as_image_copy(),
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
            let group = pass.group.get((copy_view.clone(), depth.clone()), || {
                gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("mod post"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(copy_view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                        wgpu::BindGroupEntry { binding: 2, resource: pass.uniform.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(depth) },
                    ],
                })
            });
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mod post"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(pipeline);
            rp.set_bind_group(0, &group, &[]);
            rp.draw(0..3, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gl_source_takes_x_of_depth_loads_only() {
        let src = "let a = textureLoad(depth, p + vec2<i32>(1, 0), 0);\n\
                   let b = textureLoad(scene, p, 0).rgb;\n\
                   let c = f(textureLoad( depth , q, 0), textureLoad(depth_x, q, 0));\n\
                   let d = textureLoad(scene, vec2<i32>(textureLoad(depth, q, 0) * 4.0), 0);\n\
                   let e = textureLoad (/* a */ depth // b\n , q, 0); // textureLoad(depth, q, 0)\n\
                   let f = textureLoad(depth, p + vec2<i32>(i32(textureLoad(depth, p, 0)), 0), 0);";
        assert_eq!(
            gl_source(src),
            "let a = textureLoad(depth, p + vec2<i32>(1, 0), 0).x;\n\
             let b = textureLoad(scene, p, 0).rgb;\n\
             let c = f(textureLoad( depth , q, 0).x, textureLoad(depth_x, q, 0));\n\
             let d = textureLoad(scene, vec2<i32>(textureLoad(depth, q, 0).x * 4.0), 0);\n\
             let e = textureLoad (/* a */ depth // b\n , q, 0).x; // textureLoad(depth, q, 0)\n\
             let f = textureLoad(depth, p + vec2<i32>(i32(textureLoad(depth, p, 0).x), 0), 0).x;"
        );
    }

    #[test]
    fn gl_header_keeps_the_line_count() {
        // Mods' WGSL errors count the header's lines (vana.lua: "15 header lines", with the line build joins on).
        assert_eq!(HEADER.lines().count(), HEADER_GL.lines().count());
        assert_eq!(format!("{HEADER}\n").lines().count(), 15);
    }
}
