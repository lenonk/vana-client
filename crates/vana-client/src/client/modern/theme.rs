//! The modern UI's art and style (docs/modern-ui.md): the production pack v2 (assets/modern: manifest.json with
//! logical sizes and nine-slice insets, 2x atlases, theme.json's palette), the window frames built from it, and the
//! bundled OFL fonts (assets/fonts).

use egui::{Color32, ColorImage, Context, FontData, FontDefinitions, FontFamily, Rect, TextureHandle, TextureId, TextureOptions, Vec2, pos2, vec2};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

const THEME: &str = include_str!("../../../assets/modern/theme.json");
const MANIFEST: &str = include_str!("../../../assets/modern/manifest.json");
const ATLASES: [(&str, &[u8]); 6] = [
    ("atlases/frames_0@2x.png", include_bytes!("../../../assets/modern/atlases/frames_0@2x.png")),
    ("atlases/controls_0@2x.png", include_bytes!("../../../assets/modern/atlases/controls_0@2x.png")),
    ("atlases/gauges_0@2x.png", include_bytes!("../../../assets/modern/atlases/gauges_0@2x.png")),
    ("atlases/icons_0@2x.png", include_bytes!("../../../assets/modern/atlases/icons_0@2x.png")),
    ("atlases/indicators_0@2x.png", include_bytes!("../../../assets/modern/atlases/indicators_0@2x.png")),
    ("atlases/jobs_0@2x.png", include_bytes!("../../../assets/modern/atlases/jobs_0@2x.png")),
];
const FONTS: [(&str, &[u8]); 4] = [
    ("Inter-Regular", include_bytes!("../../../assets/fonts/Inter-Regular.ttf")),
    ("Inter-SemiBold", include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf")),
    ("Inter-BoldItalic", include_bytes!("../../../assets/fonts/Inter-BoldItalic.ttf")),
    ("JetBrainsMono-Regular", include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf")),
];
/// The window frames (80 x 80 logical, 24 insets, NINE_SLICE.md); their border is cut out with the border-only
/// frame's alpha, which every variant shares.
const FRAMES: [&str; 5] = ["window_normal", "window_focused", "window_inactive", "window_alert", "window_tooltip"];
const FRAME_MASK: &str = "frames/window_normal_border";

/// Emphasis beside egui's Proportional (Inter) and Monospace (JetBrains Mono): titles and names; the bars' labels.
pub const STRONG: &str = "strong";
pub const LABEL: &str = "label";

#[derive(Deserialize, Default)]
#[serde(default)]
struct ThemeFile {
    palette: HashMap<String, String>,
}

pub struct Theme {
    colors: HashMap<String, Color32>,
    /// Point sizes: body text, titles, small print.
    pub ui: f32,
    pub title: f32,
    pub small: f32,
}

/// "#rrggbb" or "#rrggbbaa".
fn hex(s: &str) -> Option<Color32> {
    let s = s.strip_prefix('#')?;
    let v = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
    match s.len() {
        6 => Some(Color32::from_rgb(v(0)?, v(2)?, v(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(v(0)?, v(2)?, v(4)?, v(6)?)),
        _ => None,
    }
}

impl Theme {
    pub fn load() -> Self {
        let f: ThemeFile = serde_json::from_str(THEME).unwrap_or_else(|e| {
            log::error!("modern UI theme.json: {e}");
            ThemeFile::default()
        });
        let mut colors: HashMap<String, Color32> = f.palette.iter().filter_map(|(k, v)| Some((k.clone(), hex(v)?))).collect();
        // The pack's previews: parchment white text, a slate grey for the quiet parts.
        colors.entry("text".into()).or_insert(Color32::from_rgb(0xe6, 0xe1, 0xd6));
        colors.entry("muted".into()).or_insert(Color32::from_rgb(0x8f, 0x9d, 0xa6));
        Self { colors, ui: 16.0, title: 18.0, small: 14.5 }
    }

    /// A palette colour (theme.json "palette").
    pub fn color(&self, name: &str) -> Color32 {
        self.colors.get(name).copied().unwrap_or(Color32::MAGENTA)
    }

    /// Points per logical pixel: the window height against 1080 times the size setting (percent), clamped to
    /// 0.4 .. 3.
    pub fn zoom(&self, height_px: f32, scale: u32) -> f32 {
        (height_px / 1080.0 * scale as f32 / 100.0).clamp(0.4, 3.0)
    }
}

#[derive(Deserialize)]
struct ManifestFile {
    items: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    id: String,
    size: [f32; 2],
    png_size: [f32; 2],
    #[serde(default)]
    nine_slice: Option<[f32; 4]>,
    atlas: AtlasRef,
}

#[derive(Deserialize)]
struct AtlasRef {
    file: String,
    rect: [u32; 4],
}

/// A sprite in its atlas: the UV rect, its logical size, nine-slice insets in logical pixels, atlas pixels per
/// logical pixel, and the atlas size.
#[derive(Clone, Copy, Debug)]
pub struct Sprite {
    pub tex: TextureId,
    pub uv: Rect,
    pub size: Vec2,
    pub margins: Option<[f32; 4]>,
    pub px: f32,
    pub atlas: Vec2,
}

/// A window frame for any size: its border on a texture of its own (transparent inside), the interior's vertical
/// gradient (top to bottom), its logical size, corner insets and the interior's inset.
pub struct Frame {
    pub border: TextureHandle,
    pub gradient: Vec<Color32>,
    pub size: f32,
    pub corner: f32,
    pub inset: f32,
}

/// The atlases on the GPU, the sprites by manifest id, the window frames by name.
pub struct Art {
    _textures: Vec<TextureHandle>,
    sprites: HashMap<String, Sprite>,
    frames: HashMap<String, Frame>,
}

fn decode(png: &[u8]) -> Option<([usize; 2], Vec<u8>)> {
    let mut d = png::Decoder::new(std::io::Cursor::new(png));
    d.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut r = d.read_info().ok()?;
    let mut buf = vec![0u8; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    (info.color_type == png::ColorType::Rgba).then_some(([info.width as usize, info.height as usize], buf))
}

/// A rect of an RGBA image.
fn crop(img: &([usize; 2], Vec<u8>), r: [u32; 4]) -> Vec<u8> {
    let [x, y, w, h] = r.map(|v| v as usize);
    let stride = img.0[0] * 4;
    (y..y + h).flat_map(|row| img.1[row * stride + x * 4..row * stride + (x + w) * 4].iter().copied()).collect()
}

/// A window frame for the game UI's own pass (the retail windows' frames while Modern is on): the border image
/// (transparent inside), its corner as a share of its side, the interior's top and bottom colours, and the interior's
/// inset as a share of the side.
pub struct FrameImage {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
    pub corner_uv: f32,
    pub top: [f32; 4],
    pub bottom: [f32; 4],
    pub inset: f32,
}

/// The frame `name` (window_normal ...) as Art::load builds it, without egui.
pub fn frame_image(name: &str) -> Option<FrameImage> {
    let m: ManifestFile = serde_json::from_str(MANIFEST).ok()?;
    let find = |id: &str| m.items.iter().find(|it| it.id == id);
    let bytes = |file: &str| ATLASES.iter().find(|(n, _)| *n == file).map(|(_, b)| *b);
    let (mask, it) = (find(FRAME_MASK)?, find(&format!("frames/{name}"))?);
    let alpha: Vec<u8> = crop(&decode(bytes(&mask.atlas.file)?)?, mask.atlas.rect).chunks(4).map(|p| p[3]).collect();
    let (w, h) = (it.atlas.rect[2] as usize, it.atlas.rect[3] as usize);
    let mut rgba = crop(&decode(bytes(&it.atlas.file)?)?, it.atlas.rect);
    let cx = w / 2;
    let inside: Vec<usize> = (0..h).filter(|&y| rgba[(y * w + cx) * 4 + 3] > 0 && alpha.get(y * w + cx) == Some(&0)).collect();
    let (&top, &bottom) = (inside.first()?, inside.last()?);
    let col = |y: usize| {
        let k = (y * w + cx) * 4;
        [rgba[k] as f32 / 255.0, rgba[k + 1] as f32 / 255.0, rgba[k + 2] as f32 / 255.0, 1.0]
    };
    let (ct, cb) = (col(top), col(bottom));
    for (p, a) in rgba.chunks_mut(4).zip(&alpha) {
        p[3] = *a;
    }
    let corner_uv = it.nine_slice.map_or(24.0, |n| n[0]) / it.size[0];
    Some(FrameImage { size: [w, h], rgba, corner_uv, top: ct, bottom: cb, inset: top as f32 / w as f32 })
}

impl Art {
    pub fn load(ctx: &Context) -> Self {
        let mut textures = Vec::new();
        let mut images: HashMap<&str, ([usize; 2], Vec<u8>)> = HashMap::new();
        let mut atlases: HashMap<&str, (TextureId, Vec2)> = HashMap::new();
        for (name, bytes) in ATLASES {
            let Some(img) = decode(bytes) else {
                log::error!("modern UI: {name} does not decode");
                continue;
            };
            let t = ctx.load_texture(format!("modern/{name}"), ColorImage::from_rgba_unmultiplied(img.0, &img.1), TextureOptions::LINEAR);
            atlases.insert(name, (t.id(), vec2(img.0[0] as f32, img.0[1] as f32)));
            textures.push(t);
            images.insert(name, img);
        }
        let m: ManifestFile = serde_json::from_str(MANIFEST).unwrap_or_else(|e| {
            log::error!("modern UI manifest.json: {e}");
            ManifestFile { items: Vec::new() }
        });
        let mut sprites = HashMap::new();
        for it in &m.items {
            let Some(&(tex, at)) = atlases.get(it.atlas.file.as_str()) else { continue };
            let [x, y, w, h] = it.atlas.rect.map(|v| v as f32);
            let uv = Rect::from_min_max(pos2(x / at.x, y / at.y), pos2((x + w) / at.x, (y + h) / at.y));
            sprites.insert(it.id.clone(), Sprite { tex, uv, size: vec2(it.size[0], it.size[1]), margins: it.nine_slice, px: it.png_size[0] / it.size[0], atlas: at });
        }
        let mut frames = HashMap::new();
        let find = |id: &str| m.items.iter().find(|it| it.id == id);
        if let Some(mask) = find(FRAME_MASK).and_then(|it| Some((images.get(it.atlas.file.as_str())?, it))) {
            let alpha: Vec<u8> = crop(mask.0, mask.1.atlas.rect).chunks(4).map(|p| p[3]).collect();
            for name in FRAMES {
                let Some(it) = find(&format!("frames/{name}")) else { continue };
                let Some(img) = images.get(it.atlas.file.as_str()) else { continue };
                let (w, h) = (it.atlas.rect[2] as usize, it.atlas.rect[3] as usize);
                let mut rgba = crop(img, it.atlas.rect);
                // The interior: the centre column inside the border (its first and last pixels where the mask ends).
                let cx = w / 2;
                let inside: Vec<usize> = (0..h).filter(|&y| rgba[(y * w + cx) * 4 + 3] > 0 && alpha[y * w + cx] == 0).collect();
                let (Some(&top), Some(&bottom)) = (inside.first(), inside.last()) else { continue };
                let gradient = (top..=bottom).map(|y| Color32::from_rgb(rgba[(y * w + cx) * 4], rgba[(y * w + cx) * 4 + 1], rgba[(y * w + cx) * 4 + 2])).collect();
                for (p, a) in rgba.chunks_mut(4).zip(&alpha) {
                    p[3] = *a;
                }
                let px = it.png_size[0] / it.size[0];
                let border = ctx.load_texture(format!("modern/{name}"), ColorImage::from_rgba_unmultiplied([w, h], &rgba), TextureOptions::LINEAR);
                let corner = it.nine_slice.map_or(24.0, |n| n[0]);
                frames.insert(name.to_string(), Frame { border, gradient, size: it.size[0], corner, inset: top as f32 / px });
            }
        }
        Self { _textures: textures, sprites, frames }
    }

    pub fn sprite(&self, id: &str) -> Option<Sprite> {
        self.sprites.get(id).copied()
    }

    pub fn frame(&self, name: &str) -> Option<&Frame> {
        self.frames.get(name)
    }
}

/// The bundled fonts: Inter for the interface (egui's Proportional), Inter SemiBold for titles and names (STRONG),
/// A bundled font's bytes by name ("Inter-BoldItalic" ...).
pub fn font(name: &str) -> Option<&'static [u8]> {
    FONTS.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

/// Inter Bold Italic for the bars' labels (LABEL), JetBrains Mono for logs (Monospace); egui's own fonts stay behind
/// them for missing glyphs.
pub fn install_fonts(ctx: &Context) {
    let mut defs = FontDefinitions::default();
    for (name, bytes) in FONTS {
        defs.font_data.insert(name.to_string(), Arc::new(FontData::from_static(bytes)));
    }
    let fallbacks = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let with = |first: &str| -> Vec<String> { std::iter::once(first.to_string()).chain(fallbacks.iter().cloned()).collect() };
    defs.families.insert(FontFamily::Proportional, with("Inter-Regular"));
    defs.families.insert(FontFamily::Name(STRONG.into()), with("Inter-SemiBold"));
    defs.families.insert(FontFamily::Name(LABEL.into()), with("Inter-BoldItalic"));
    let mono = defs.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    defs.families.insert(FontFamily::Monospace, std::iter::once("JetBrainsMono-Regular".to_string()).chain(mono).collect());
    ctx.set_fonts(defs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_and_manifest_parse() {
        let t = Theme::load();
        for c in ["charcoal", "surface", "bronze", "gold", "focus", "hp", "mp", "tp", "text", "muted"] {
            assert_ne!(t.color(c), Color32::MAGENTA, "{c}");
        }
        let m: ManifestFile = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!(m.items.len(), 304);
        assert!(m.items.iter().all(|it| ATLASES.iter().any(|(n, _)| *n == it.atlas.file)));
        for (_, bytes) in ATLASES {
            assert!(decode(bytes).is_some());
        }
        for f in FRAMES.iter().map(|f| format!("frames/{f}")).chain([FRAME_MASK.to_string()]) {
            assert!(m.items.iter().any(|it| it.id == f && it.nine_slice == Some([24.0; 4])), "{f}");
        }
    }
}
