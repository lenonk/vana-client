//! The mods' HUD layer (docs/lua-mods.md 4.10): retained text, rectangles, bars and icons placed in window pixels
//! (negative from the right or bottom edge), drawn under the windows in the active UI's fonts. Hidden with the UI.

use super::World;
use super::super::game_ui::HudFace;
use mlua::{Lua, Table, Value};

#[derive(Clone)]
pub(super) enum Kind {
    /// `font` None follows the UI; an outline (its colour and width) or a drop shadow under the text.
    Text { text: String, size: f32, colour: Option<[u8; 4]>, bg: Option<[u8; 4]>, align: u8, pad: f32, font: Option<HudFace>, outline: Option<[u8; 4]>, outline_w: f32, shadow: bool },
    Rect { w: f32, h: f32, colour: [u8; 4] },
    Bar { w: f32, h: f32, value: f32, colour: Option<[u8; 4]>, kind: String },
    Icon { item: bool, id: u16, size: f32 },
    /// A PNG from the mod's folder: its texture key, pixels (w, h, RGBA), drawn size and tint.
    Image { key: String, img: std::sync::Arc<(u32, u32, Vec<u8>)>, w: f32, h: f32, tint: [u8; 4] },
    /// The zone's map (the one the client shows here): centred on world (x, z) or on us, `span` map pixels across.
    Map { w: f32, h: f32, centre: Option<(f32, f32)>, span: f32, alpha: f32 },
}

#[derive(Clone)]
pub(super) struct Elem {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub visible: bool,
    pub layer: i32,
    pub kind: Kind,
}

pub(super) fn colour(v: Value) -> mlua::Result<Option<[u8; 4]>> {
    Ok(match v {
        Value::Table(t) => {
            let get = |k: &str, i: i64, d: u8| -> mlua::Result<u8> { Ok(t.get::<Option<u8>>(k)?.or(t.get::<Option<u8>>(i)?).unwrap_or(d)) };
            Some([get("r", 1, 255)?, get("g", 2, 255)?, get("b", 3, 255)?, get("a", 4, 255)?])
        }
        // false clears a colour set before (a nil field is not seen).
        Value::Nil | Value::Boolean(false) => None,
        _ => return Err(mlua::Error::runtime("a colour is {r, g, b, a} (0..255), or false for none")),
    })
}

/// Applies a spec's fields to an element (only those given).
fn apply(e: &mut Elem, t: &Table) -> mlua::Result<()> {
    if let Some(v) = t.get::<Option<f32>>("x")? {
        e.x = v;
    }
    if let Some(v) = t.get::<Option<f32>>("y")? {
        e.y = v;
    }
    if let Some(v) = t.get::<Option<bool>>("visible")? {
        e.visible = v;
    }
    if let Some(v) = t.get::<Option<i32>>("layer")? {
        e.layer = v;
    }
    match &mut e.kind {
        Kind::Text { text, size, colour: c, bg, align, pad, font, outline, outline_w, shadow } => {
            if let Some(v) = t.get::<Option<String>>("text")? {
                *text = v;
            }
            if let Some(v) = t.get::<Option<f32>>("size")? {
                *size = v.clamp(0.25, 8.0);
            }
            if t.contains_key("colour")? {
                *c = colour(t.get("colour")?)?;
            }
            if t.contains_key("bg")? {
                *bg = colour(t.get("bg")?)?;
            }
            if let Some(v) = t.get::<Option<String>>("align")? {
                *align = match v.as_str() {
                    "center" | "centre" => 1,
                    "right" => 2,
                    _ => 0,
                };
            }
            if let Some(v) = t.get::<Option<f32>>("pad")? {
                *pad = v.max(0.0);
            }
            if let Some(v) = t.get::<Option<String>>("font")? {
                *font = match v.to_ascii_lowercase().as_str() {
                    "game" => Some(HudFace::Game),
                    "regular" => Some(HudFace::Regular),
                    "bold" => Some(HudFace::Bold),
                    // Any other name follows the UI's own face.
                    _ => None,
                };
            }
            if t.contains_key("outline")? {
                *outline = colour(t.get("outline")?)?;
            }
            if let Some(v) = t.get::<Option<f32>>("outline_width")? {
                *outline_w = v.clamp(0.5, 4.0);
            }
            if let Some(v) = t.get::<Option<bool>>("shadow")? {
                *shadow = v;
            }
        }
        Kind::Rect { w, h, colour: c } => {
            if let Some(v) = t.get::<Option<f32>>("w")? {
                *w = v;
            }
            if let Some(v) = t.get::<Option<f32>>("h")? {
                *h = v;
            }
            if let Some(v) = colour(t.get("colour")?)? {
                *c = v;
            }
        }
        Kind::Bar { w, h, value, colour: c, kind } => {
            if let Some(v) = t.get::<Option<f32>>("w")? {
                *w = v;
            }
            if let Some(v) = t.get::<Option<f32>>("h")? {
                *h = v;
            }
            if let Some(v) = t.get::<Option<f32>>("value")? {
                *value = v.clamp(0.0, 1.0);
            }
            if t.contains_key("colour")? {
                *c = colour(t.get("colour")?)?;
            }
            if let Some(v) = t.get::<Option<String>>("kind")? {
                *kind = v;
            }
        }
        Kind::Map { w, h, centre, span, alpha } => {
            if let Some(v) = t.get::<Option<f32>>("w")? {
                *w = v;
            }
            if let Some(v) = t.get::<Option<f32>>("h")? {
                *h = v;
            }
            if t.contains_key("centre")? {
                *centre = match t.get::<Option<Table>>("centre")? {
                    Some(c) => Some((c.get("x")?, c.get("z")?)),
                    None => None,
                };
            }
            if let Some(v) = t.get::<Option<f32>>("span")? {
                *span = v.clamp(16.0, 2048.0);
            }
            if let Some(v) = t.get::<Option<f32>>("alpha")? {
                *alpha = v.clamp(0.0, 1.0);
            }
        }
        Kind::Image { w, h, tint, .. } => {
            if let Some(v) = t.get::<Option<f32>>("w")? {
                *w = v;
            }
            if let Some(v) = t.get::<Option<f32>>("h")? {
                *h = v;
            }
            if let Some(v) = colour(t.get("colour")?)? {
                *tint = v;
            }
        }
        Kind::Icon { item, id, size } => {
            // As list and canvas icons: kind = "status" for a status icon, else an item's.
            if let Some(v) = t.get::<Option<String>>("kind")? {
                *item = v != "status";
            }
            if let Some(v) = t.get::<Option<u16>>("id")? {
                *id = v;
            }
            if let Some(v) = t.get::<Option<f32>>("size")? {
                *size = v.max(1.0);
            }
        }
    }
    Ok(())
}

fn put(lua: &Lua, id: String, kind: Kind, spec: Table) -> mlua::Result<()> {
    let mut e = Elem { id: id.clone(), x: 0.0, y: 0.0, visible: true, layer: 0, kind };
    apply(&mut e, &spec)?;
    let st = super::api::state(lua);
    let mut s = st.borrow_mut();
    match s.hud.iter_mut().find(|x| x.id == id) {
        Some(old) => *old = e,
        None => s.hud.push(e),
    }
    Ok(())
}

/// A PNG from the mod's folder: its texture key (with the file's hash, so a changed file is drawn anew) and pixels.
pub(super) fn load_png(src: &super::vm::Source, mod_id: &str, path: &str) -> mlua::Result<(String, std::sync::Arc<(u32, u32, Vec<u8>)>)> {
    if path.contains("..") || path.starts_with('/') || path.contains(':') {
        return Err(mlua::Error::runtime(format!("{path:?}: outside the mod's folder")));
    }
    let bytes = match src {
        super::vm::Source::Dir(d) => super::vm::inside(d, path).and_then(std::fs::read).map_err(|e| mlua::Error::runtime(format!("{path}: {e}")))?,
        super::vm::Source::Bundled(_) => return Err(mlua::Error::runtime("bundled mods carry no image files")),
    };
    let img = decode_png(&bytes).map_err(|e| mlua::Error::runtime(format!("{path}: {e} (a PNG)")))?;
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    Ok((format!("lua image {mod_id}/{path}#{:016x}", h.finish()), std::sync::Arc::new(img)))
}

/// A PNG's pixels as RGBA.
pub(super) fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut d = png::Decoder::new(std::io::Cursor::new(bytes));
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = d.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; r.output_buffer_size().unwrap_or(0)];
    let info = r.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width, info.height);
    let n = (w * h) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..n * 4].to_vec(),
        png::ColorType::Rgb => buf.chunks(3).take(n).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks(2).take(n).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().take(n).flat_map(|&p| [p, p, p, 255]).collect(),
        other => return Err(format!("colour type {other:?}")),
    };
    Ok((w, h, rgba))
}

/// The texture key of a mod's image: its file and a hash of the file's bytes, so a changed file (the mod reloaded)
/// uploads anew instead of drawing the texture cached for the old one.
pub(super) fn image_key(mod_id: &str, path: &str, bytes: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    format!("lua image {mod_id}/{path}#{:016x}", h.finish())
}

pub(super) fn install(lua: &Lua, g: &Table, vm: &super::vm::Vm) -> mlua::Result<()> {
    let t = lua.create_table()?;
    let (src, mod_id) = (vm.src.clone(), vm.manifest.id.clone());
    t.set(
        "image",
        lua.create_function(move |lua, (id, spec): (String, Table)| {
            let path: String = spec.get("path")?;
            let (key, img) = load_png(&src, &mod_id, &path)?;
            let (iw, ih) = (img.0 as f32, img.1 as f32);
            put(lua, id, Kind::Image { key, img, w: iw, h: ih, tint: [255; 4] }, spec)
        })?,
    )?;
    let (src2, mod2) = (vm.src.clone(), vm.manifest.id.clone());
    let chat: Table = g.get("chat")?;
    chat.set(
        "icon",
        lua.create_function(move |_, path: String| {
            if path.contains("..") || path.starts_with('/') || path.contains(':') {
                return Err(mlua::Error::runtime(format!("{path:?}: outside the mod's folder")));
            }
            let bytes = match &src2 {
                super::vm::Source::Dir(d) => super::vm::inside(d, &path).and_then(std::fs::read).map_err(|e| mlua::Error::runtime(format!("{path}: {e}")))?,
                super::vm::Source::Bundled(_) => return Err(mlua::Error::runtime("bundled mods carry no image files")),
            };
            let (w, h, rgba) = decode_png(&bytes).map_err(|e| mlua::Error::runtime(format!("{path}: {e} (a PNG)")))?;
            let c = super::super::chaticons::add(&format!("{mod2}/{path}"), w, h, rgba).ok_or_else(|| mlua::Error::runtime("too many chat icons"))?;
            Ok(c.to_string())
        })?,
    )?;
    t.set("text", lua.create_function(|lua, (id, spec): (String, Table)| put(lua, id, Kind::Text { text: String::new(), size: 1.0, colour: None, bg: None, align: 0, pad: 4.0, font: None, outline: None, outline_w: 1.0, shadow: false }, spec))?)?;
    t.set("rect", lua.create_function(|lua, (id, spec): (String, Table)| put(lua, id, Kind::Rect { w: 100.0, h: 20.0, colour: [0, 0, 0, 160] }, spec))?)?;
    t.set("bar", lua.create_function(|lua, (id, spec): (String, Table)| put(lua, id, Kind::Bar { w: 120.0, h: 8.0, value: 0.0, colour: None, kind: "hp".into() }, spec))?)?;
    t.set("icon", lua.create_function(|lua, (id, spec): (String, Table)| put(lua, id, Kind::Icon { item: true, id: 0, size: 32.0 }, spec))?)?;
    t.set("map", lua.create_function(|lua, (id, spec): (String, Table)| put(lua, id, Kind::Map { w: 200.0, h: 200.0, centre: None, span: 256.0, alpha: 1.0 }, spec))?)?;
    let (src3, mod3) = (vm.src.clone(), vm.manifest.id.clone());
    t.set(
        "set",
        lua.create_function(move |lua, (id, fields): (String, Table)| {
            let png = fields.get::<Option<String>>("path")?.map(|p| load_png(&src3, &mod3, &p)).transpose()?;
            let st = super::api::state(lua);
            let mut s = st.borrow_mut();
            let e = s.hud.iter_mut().find(|x| x.id == id).ok_or_else(|| mlua::Error::runtime(format!("no HUD element {id:?}")))?;
            // A new picture: its own size unless the fields give one.
            if let Some((k, i)) = png {
                let Kind::Image { key, img, w, h, .. } = &mut e.kind else { return Err(mlua::Error::runtime("path is an image's field")) };
                (*w, *h) = (i.0 as f32, i.1 as f32);
                (*key, *img) = (k, i);
            }
            apply(e, &fields)
        })?,
    )?;
    // An element's click handler goes with it.
    let off = |lua: &Lua, which: &dyn Fn(&str) -> bool| -> mlua::Result<()> {
        let gone: Vec<mlua::RegistryKey> = {
            let st = super::api::state(lua);
            let mut s = st.borrow_mut();
            let (gone, keep) = std::mem::take(&mut s.hud_handlers).into_iter().partition(|h| which(&h.0));
            s.hud_handlers = keep;
            gone.into_iter().map(|h| h.1).collect()
        };
        for k in gone {
            lua.remove_registry_value(k)?;
        }
        Ok(())
    };
    t.set(
        "remove",
        lua.create_function(move |lua, id: String| {
            super::api::state(lua).borrow_mut().hud.retain(|x| x.id != id);
            off(lua, &|h| h == id)
        })?,
    )?;
    t.set(
        "clear",
        lua.create_function(move |lua, ()| {
            super::api::state(lua).borrow_mut().hud.clear();
            off(lua, &|_| true)
        })?,
    )?;
    t.set("off", lua.create_function(move |lua, id: String| off(lua, &|h| h == id))?)?;
    t.set(
        "on",
        lua.create_function(|lua, (id, func): (String, mlua::Function)| {
            let st = super::api::state(lua);
            let key = lua.create_registry_value(func)?;
            let old = {
                let mut s = st.borrow_mut();
                let old = s.hud_handlers.iter().position(|h| h.0 == id).map(|k| s.hud_handlers.remove(k).1);
                s.hud_handlers.push((id, key));
                old
            };
            if let Some(k) = old {
                lua.remove_registry_value(k)?;
            }
            Ok(())
        })?,
    )?;
    t.set(
        "screen",
        lua.create_function(|lua, ()| {
            let info = lua.app_data_ref::<std::rc::Rc<std::cell::RefCell<super::state::FrameInfo>>>().map(|f| f.clone()).unwrap_or_default();
            let f = info.borrow();
            let out = lua.create_table()?;
            out.set("width", f.window[0] as i64)?;
            out.set("height", f.window[1] as i64)?;
            out.set("ui_scale", if f.k.0 > 0.0 { 1.0 / f.k.0 } else { 1.0 })?;
            Ok(out)
        })?,
    )?;
    t.set(
        "bounds",
        lua.create_function(|lua, id: String| {
            let info = lua.app_data_ref::<std::rc::Rc<std::cell::RefCell<super::state::FrameInfo>>>().map(|f| f.clone()).unwrap_or_default();
            let mod_id = lua.globals().get::<Table>("mod")?.get::<String>("id")?;
            let f = info.borrow();
            match f.hud_rects.iter().find(|r| r.0 == mod_id && r.1 == id) {
                Some(r) => {
                    let out = lua.create_table()?;
                    out.set("x", r.2[0].round() as i64)?;
                    out.set("y", r.2[1].round() as i64)?;
                    out.set("w", r.2[2].round() as i64)?;
                    out.set("h", r.2[3].round() as i64)?;
                    Ok(Some(out))
                }
                None => Ok(None),
            }
        })?,
    )?;
    g.set("hud", t)?;
    Ok(())
}

/// The game font's colour (BGRA, 0x80 = 1.0) of an RGBA colour.
pub(super) fn tint(c: [u8; 4]) -> [u8; 4] {
    [c[2] / 2 + c[2] % 2, c[1] / 2 + c[1] % 2, c[0] / 2 + c[0] % 2, c[3] / 2 + c[3] % 2]
}

fn rgba(c: [u8; 4]) -> [f32; 4] {
    c.map(|v| v as f32 / 255.0)
}

fn bar_colour(kind: &str) -> [u8; 4] {
    match kind {
        "mp" => [200, 120, 70, 255],
        "tp" => [80, 160, 220, 255],
        "cast" => [90, 210, 210, 255],
        "experience" => [200, 110, 210, 255],
        "recast" => [160, 160, 160, 255],
        _ => [90, 200, 90, 255],
    }
}

/// Every running mod's visible elements, by layer then load order, at UI units (window pixels times `k`). Their window
/// pixel rects go to the frame info for clicks and hud.bounds.
pub(super) fn draw(gu: &mut super::super::game_ui::GameUi, w: &World) {
    let host = &w.lua;
    let info = host.info.clone();
    let (k, win) = {
        let f = info.borrow();
        (f.k, f.window)
    };
    if k.0 <= 0.0 || k.1 <= 0.0 {
        return;
    }
    let mut all: Vec<(String, Elem)> = Vec::new();
    for vm in host.vms.iter().filter(|v| v.suspended.is_none()) {
        for e in vm.st.borrow().hud.iter().filter(|e| e.visible) {
            all.push((vm.manifest.id.clone(), e.clone()));
        }
    }
    all.sort_by_key(|(_, e)| e.layer);
    let mut rects = Vec::new();
    for (mod_id, e) in all {
        // Negative places count from the right or bottom edge.
        let px = if e.x < 0.0 { win[0] + e.x } else { e.x };
        let py = if e.y < 0.0 { win[1] + e.y } else { e.y };
        let (x, y) = (px * k.0, py * k.1);
        let r = match &e.kind {
            Kind::Text { text, size, colour, bg, align, pad, font, outline, outline_w, shadow } => {
                let lines: Vec<&str> = text.split('\n').collect();
                let lw = lines.iter().map(|l| gu.width_in(l, *font) * size).fold(0.0, f32::max);
                let lh = 16.0 * size;
                let th = lh * lines.len() as f32;
                let left = match align {
                    1 => x - lw / 2.0,
                    2 => x - lw,
                    _ => x,
                };
                if let Some(b) = bg {
                    gu.rect(left - pad, y - pad / 2.0, lw + pad * 2.0, th + *pad, rgba(*b));
                }
                let c = tint(colour.unwrap_or([255, 255, 255, 255]));
                for (n, l) in lines.iter().enumerate() {
                    let ly = y + lh * n as f32;
                    // An outline drawn round the text in 8 directions, or a drop shadow, under it.
                    if let Some(o) = outline {
                        let (oc, d) = (tint(*o), outline_w * size.max(1.0));
                        for (dx, dy) in [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0)] {
                            gu.text_in(l, left + dx * d, ly + dy * d, oc, *size, *font);
                        }
                    }
                    if *shadow {
                        let d = 1.5 * size.max(1.0);
                        gu.text_in(l, left + d, ly + d, tint([0, 0, 0, 200]), *size, *font);
                    }
                    gu.text_in(l, left, ly, c, *size, *font);
                }
                let p = if bg.is_some() { *pad } else { 0.0 };
                [left - p, y - p / 2.0, lw + p * 2.0, th + p]
            }
            Kind::Rect { w: rw, h: rh, colour } => {
                let (rw, rh) = (rw * k.0, rh * k.1);
                gu.rect(x, y, rw, rh, rgba(*colour));
                [x, y, rw, rh]
            }
            Kind::Bar { w: bw, h: bh, value, colour, kind } => {
                let (bw, bh) = (bw * k.0, bh * k.1);
                gu.rect(x, y, bw, bh, [0.0, 0.0, 0.0, 0.55]);
                if *value > 0.0 {
                    gu.rect(x + 1.0, y + 1.0, (bw - 2.0).max(0.0) * value, (bh - 2.0).max(0.0), rgba(colour.unwrap_or_else(|| bar_colour(kind))));
                }
                [x, y, bw, bh]
            }
            Kind::Map { w: mw, h: mh, centre, span, alpha } => {
                let (mw, mh) = (mw * k.0, mh * k.1);
                let regions = gu.map_regions().to_vec();
                let mut m = w.lua.map.borrow_mut();
                match m.as_mut() {
                    Some(o) if o.zone == w.pos_zone => o.follow(w, &regions),
                    _ => *m = Some(super::super::map::Overlay::open(w, &regions)),
                }
                if let Some(o) = m.as_mut() {
                    o.draw_window(gu, [x, y, mw, mh], centre.unwrap_or((w.pos.x, w.pos.z)), *span, *alpha);
                }
                [x, y, mw, mh]
            }
            Kind::Image { key, img, w: iw, h: ih, tint } => {
                if !gu.has_image(key) {
                    gu.image(key, img.0, img.1, img.2.clone());
                }
                let (iw, ih) = (iw * k.0, ih * k.1);
                gu.image_rect(key, x, y, iw, ih, tint.map(|c| c as f32 / 255.0));
                [x, y, iw, ih]
            }
            Kind::Icon { item, id, size } => {
                let s = size * k.0;
                if *item {
                    if let Some(cache) = w.item_info.as_ref() {
                        super::super::items::upload_icons(gu, cache, std::iter::once(*id));
                        let icon = crate::render::invwin::icon_texture(*id);
                        if gu.has_image(&icon) {
                            gu.image_rect(&icon, x, y, s, s, [1.0; 4]);
                        }
                    }
                } else if let Some(name) = gu.status_icon(false, *id) {
                    gu.image_rect(&name, x, y, s, s, [1.0; 4]);
                }
                [x, y, s, s]
            }
        };
        rects.push((mod_id, e.id.clone(), [r[0] / k.0, r[1] / k.1, r[2] / k.0, r[3] / k.1]));
    }
    info.borrow_mut().hud_rects = rects;
}

/// The topmost element with a click handler under a window pixel: (mod, element).
pub(super) fn hit(w: &World, x: f64, y: f64) -> Option<(String, String)> {
    let f = w.lua.info.borrow();
    let (x, y) = (x as f32, y as f32);
    f.hud_rects.iter().rev().find(|(m, id, r)| {
        x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3] && w.lua.vms.iter().any(|v| v.manifest.id == *m && v.suspended.is_none() && v.st.borrow().hud_handlers.iter().any(|h| h.0 == *id))
    }).map(|(m, id, _)| (m.clone(), id.clone()))
}
