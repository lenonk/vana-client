//! The modern UI's art and style (docs/modern-ui.md): theme.json (colours, type sizes, scale, metrics), manifest.json
//! (sprite rects and nine-slice margins in the packed atlases), the atlases as egui textures, and the bundled OFL
//! fonts (assets/fonts).

use egui::{Color32, Context, FontData, FontDefinitions, FontFamily, Rect, TextureHandle, TextureId, TextureOptions, Vec2, pos2, vec2};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

const THEME: &str = include_str!("../../../assets/modern/theme.json");
const MANIFEST: &str = include_str!("../../../assets/modern/manifest.json");
const ATLASES: [(&str, &[u8]); 6] = [
    ("atlases/nine_slice.png", include_bytes!("../../../assets/modern/atlases/nine_slice.png")),
    ("atlases/gauge.png", include_bytes!("../../../assets/modern/atlases/gauge.png")),
    ("atlases/icon.png", include_bytes!("../../../assets/modern/atlases/icon.png")),
    ("atlases/sprite.png", include_bytes!("../../../assets/modern/atlases/sprite.png")),
    ("atlases/overlay.png", include_bytes!("../../../assets/modern/atlases/overlay.png")),
    ("atlases/keycap.png", include_bytes!("../../../assets/modern/atlases/keycap.png")),
];
const FONTS: [(&str, &[u8]); 5] = [
    ("Inter-Regular", include_bytes!("../../../assets/fonts/Inter-Regular.ttf")),
    ("Inter-SemiBold", include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf")),
    ("Cinzel-Regular", include_bytes!("../../../assets/fonts/Cinzel-Regular.ttf")),
    ("Cinzel-Bold", include_bytes!("../../../assets/fonts/Cinzel-Bold.ttf")),
    ("JetBrainsMono-Regular", include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf")),
];

/// Type roles beside egui's Proportional (Inter) and Monospace (JetBrains Mono): window titles and emphasis.
pub const DISPLAY: &str = "display";
pub const STRONG: &str = "strong";

#[derive(Deserialize, Default)]
#[serde(default)]
struct ThemeFile {
    colors: HashMap<String, String>,
    fonts: HashMap<String, FontRole>,
    scale: Scale,
    metrics: Metrics,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct FontRole {
    size: f32,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default)]
pub struct Scale {
    pub reference_height: f32,
    pub user_scale: f32,
    pub min_scale: f32,
    pub max_scale: f32,
}

impl Default for Scale {
    fn default() -> Self {
        Self { reference_height: 1080.0, user_scale: 1.0, min_scale: 0.75, max_scale: 2.5 }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Metrics {
    pub row_height: f32,
    pub icon_size: f32,
    pub menu_width: f32,
    pub drag_threshold_px: f32,
    pub double_click_ms: u32,
}

pub struct Theme {
    colors: HashMap<String, Color32>,
    /// Point sizes of the display, interface and log roles.
    pub display: f32,
    pub ui: f32,
    pub log: f32,
    pub scale: Scale,
    pub metrics: Metrics,
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
        let colors = f.colors.iter().filter_map(|(k, v)| Some((k.clone(), hex(v)?))).collect();
        let size = |role: &str, d: f32| f.fonts.get(role).map_or(d, |r| r.size).max(6.0);
        Self { colors, display: size("display", 22.0), ui: size("ui", 15.0), log: size("log", 13.0), scale: f.scale, metrics: f.metrics }
    }

    /// A named theme colour (theme.json "colors").
    pub fn color(&self, name: &str) -> Color32 {
        self.colors.get(name).copied().unwrap_or(Color32::MAGENTA)
    }

    /// Points per UI unit: the window height against the reference height, clamped (theme.json "scale").
    pub fn zoom(&self, height_px: f32) -> f32 {
        let s = self.scale;
        (height_px / s.reference_height.max(1.0) * s.user_scale).clamp(s.min_scale, s.max_scale)
    }
}

#[derive(Deserialize)]
struct ManifestFile {
    assets: Vec<AssetEntry>,
}

#[derive(Deserialize)]
struct AssetEntry {
    name: String,
    size: [u32; 2],
    #[serde(default)]
    nine_slice: Option<[u32; 4]>,
    atlas: AtlasRef,
}

#[derive(Deserialize)]
struct AtlasRef {
    image: String,
    rect: [u32; 4],
}

/// A sprite's place in its atlas: the texture, its UV rect, its size in source pixels, and for a nine-slice its
/// margins (left, top, right, bottom) in source pixels.
#[derive(Clone, Copy, Debug)]
pub struct Sprite {
    pub tex: TextureId,
    pub uv: Rect,
    pub size: Vec2,
    pub margins: Option<[f32; 4]>,
    /// The atlas size, to turn source pixels into UV.
    pub atlas: Vec2,
}

/// The atlases on the GPU and the manifest's sprites by name.
pub struct Art {
    _textures: Vec<TextureHandle>,
    sprites: HashMap<String, Sprite>,
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

impl Art {
    pub fn load(ctx: &Context) -> Self {
        let mut textures = Vec::new();
        let mut atlases: HashMap<&str, (TextureId, Vec2)> = HashMap::new();
        for (name, bytes) in ATLASES {
            let Some((size, rgba)) = decode(bytes) else {
                log::error!("modern UI: {name} does not decode");
                continue;
            };
            let t = ctx.load_texture(format!("modern/{name}"), egui::ColorImage::from_rgba_unmultiplied(size, &rgba), TextureOptions::LINEAR);
            atlases.insert(name, (t.id(), vec2(size[0] as f32, size[1] as f32)));
            textures.push(t);
        }
        let m: ManifestFile = serde_json::from_str(MANIFEST).unwrap_or_else(|e| {
            log::error!("modern UI manifest.json: {e}");
            ManifestFile { assets: Vec::new() }
        });
        let mut sprites = HashMap::new();
        for a in m.assets {
            let Some(&(tex, at)) = atlases.get(a.atlas.image.as_str()) else { continue };
            let [x, y, w, h] = a.atlas.rect.map(|v| v as f32);
            let uv = Rect::from_min_max(pos2(x / at.x, y / at.y), pos2((x + w) / at.x, (y + h) / at.y));
            let margins = a.nine_slice.map(|m| m.map(|v| v as f32));
            sprites.insert(a.name, Sprite { tex, uv, size: vec2(a.size[0] as f32, a.size[1] as f32), margins, atlas: at });
        }
        Self { _textures: textures, sprites }
    }

    pub fn sprite(&self, name: &str) -> Option<Sprite> {
        self.sprites.get(name).copied()
    }
}

/// The bundled fonts: Inter for the interface (egui's Proportional), JetBrains Mono for logs (Monospace), Cinzel Bold
/// for titles (DISPLAY), Inter SemiBold for emphasis (STRONG); egui's own fonts stay behind them for missing glyphs.
pub fn install_fonts(ctx: &Context) {
    let mut defs = FontDefinitions::default();
    for (name, bytes) in FONTS {
        defs.font_data.insert(name.to_string(), Arc::new(FontData::from_static(bytes)));
    }
    let fallbacks = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let with = |first: &str| -> Vec<String> { std::iter::once(first.to_string()).chain(fallbacks.iter().cloned()).collect() };
    defs.families.insert(FontFamily::Proportional, with("Inter-Regular"));
    defs.families.insert(FontFamily::Name(DISPLAY.into()), with("Cinzel-Bold"));
    defs.families.insert(FontFamily::Name(STRONG.into()), with("Inter-SemiBold"));
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
        for c in ["bg", "panel", "edge", "accent", "focus", "text", "muted", "danger", "good", "blue", "gold"] {
            assert_ne!(t.color(c), Color32::MAGENTA, "{c}");
        }
        assert_eq!((t.display, t.ui, t.log), (22.0, 15.0, 13.0));
        let m: ManifestFile = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!(m.assets.len(), 196);
        assert!(m.assets.iter().all(|a| ATLASES.iter().any(|(n, _)| *n == a.atlas.image)));
        for (_, bytes) in ATLASES {
            assert!(decode(bytes).is_some());
        }
    }
}
