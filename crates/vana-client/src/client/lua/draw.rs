//! World-space drawing for mods (docs/lua-mods.md 4.10 "World space"): lines, rings and boxes in world coordinates (as
//! entity positions are), behind whatever stands in front of them, and world-to-screen for labels.

use super::World;
use crate::render::LineVertex;
use glam::Vec3;
use mlua::{Lua, Table, Value};

#[derive(Clone)]
pub(super) enum Kind {
    Line(Vec3, Vec3),
    /// Centre, radius, segments, filled.
    Ring(Vec3, f32, u32, bool),
    /// Centre and size (an axis-aligned wire box).
    Box(Vec3, Vec3),
    /// Text at a point, drawn with the HUD (centred, behind what stands in front).
    Text { at: Vec3, text: String, size: f32, outline: Option<[u8; 4]> },
    /// A PNG of the mod's on a quad `w` x `h` centred at a point: on the ground, upright or turned to the camera,
    /// turned by `yaw` (radians, 0 east as entity headings).
    Image { at: Vec3, w: f32, h: f32, yaw: f32, facing: Facing, key: String, img: std::sync::Arc<(u32, u32, Vec<u8>)> },
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Facing {
    Ground,
    Upright,
    Camera,
}

#[derive(Clone)]
pub(super) struct Shape {
    pub id: String,
    pub kind: Kind,
    /// RGBA 0..255.
    pub colour: [u8; 4],
}

fn vec3(v: Value) -> mlua::Result<Vec3> {
    match v {
        Value::Table(t) => Ok(Vec3::new(t.get::<Option<f32>>("x")?.or(t.get(1)?).unwrap_or(0.0), t.get::<Option<f32>>("y")?.or(t.get(2)?).unwrap_or(0.0), t.get::<Option<f32>>("z")?.or(t.get(3)?).unwrap_or(0.0))),
        _ => Err(mlua::Error::runtime("a point is {x, y, z}")),
    }
}

fn colour(t: &Table) -> mlua::Result<[u8; 4]> {
    Ok(match t.get::<Value>("colour")? {
        Value::Table(c) => {
            let get = |k: &str, i: i64, d: u8| -> mlua::Result<u8> { Ok(c.get::<Option<u8>>(k)?.or(c.get::<Option<u8>>(i)?).unwrap_or(d)) };
            [get("r", 1, 255)?, get("g", 2, 255)?, get("b", 3, 255)?, get("a", 4, 255)?]
        }
        _ => [255, 255, 255, 255],
    })
}

fn put(lua: &Lua, id: String, kind: Kind, colour: [u8; 4]) -> mlua::Result<()> {
    let st = super::api::state(lua);
    let mut s = st.borrow_mut();
    let shape = Shape { id: id.clone(), kind, colour };
    match s.shapes.iter_mut().find(|x| x.id == id) {
        Some(old) => *old = shape,
        None => s.shapes.push(shape),
    }
    Ok(())
}

pub(super) fn install(lua: &Lua, g: &Table, vm: &super::vm::Vm) -> mlua::Result<()> {
    let t = lua.create_table()?;
    t.set(
        "text",
        lua.create_function(|lua, (id, s): (String, Table)| {
            let kind = Kind::Text { at: vec3(s.get("at")?)?, text: s.get::<Option<String>>("text")?.unwrap_or_default(), size: s.get::<Option<f32>>("size")?.unwrap_or(1.0).clamp(0.25, 8.0), outline: super::hud::colour(s.get("outline")?)? };
            put(lua, id, kind, colour(&s)?)
        })?,
    )?;
    let (src, mod_id) = (vm.src.clone(), vm.manifest.id.clone());
    t.set(
        "image",
        lua.create_function(move |lua, (id, s): (String, Table)| {
            let path: String = s.get("path")?;
            if path.contains("..") || path.starts_with('/') || path.contains(':') {
                return Err(mlua::Error::runtime(format!("{path:?}: outside the mod's folder")));
            }
            let bytes = match &src {
                super::vm::Source::Dir(d) => std::fs::read(d.join(&path)).map_err(|e| mlua::Error::runtime(format!("{path}: {e}")))?,
                super::vm::Source::Bundled(_) => return Err(mlua::Error::runtime("bundled mods carry no image files")),
            };
            let img = super::hud::decode_png(&bytes).map_err(|e| mlua::Error::runtime(format!("{path}: {e} (a PNG)")))?;
            let facing = match s.get::<Option<String>>("facing")?.as_deref() {
                Some("upright") => Facing::Upright,
                Some("camera") => Facing::Camera,
                _ => Facing::Ground,
            };
            let (w, h) = (s.get::<Option<f32>>("w")?.unwrap_or(2.0).max(0.01), s.get::<Option<f32>>("h")?.unwrap_or(2.0).max(0.01));
            let kind = Kind::Image { at: vec3(s.get("at")?)?, w, h, yaw: s.get::<Option<f32>>("yaw")?.unwrap_or(0.0), facing, key: format!("lua image {mod_id}/{path}"), img: std::sync::Arc::new(img) };
            put(lua, id, kind, colour(&s)?)
        })?,
    )?;
    t.set("line", lua.create_function(|lua, (id, s): (String, Table)| put(lua, id, Kind::Line(vec3(s.get("from")?)?, vec3(s.get("to")?)?), colour(&s)?))?)?;
    t.set(
        "ring",
        lua.create_function(|lua, (id, s): (String, Table)| {
            let segments = s.get::<Option<u32>>("segments")?.unwrap_or(48).clamp(3, 360);
            let kind = Kind::Ring(vec3(s.get("at")?)?, s.get::<Option<f32>>("radius")?.unwrap_or(1.0).max(0.0), segments, s.get::<Option<bool>>("filled")?.unwrap_or(false));
            put(lua, id, kind, colour(&s)?)
        })?,
    )?;
    t.set("box", lua.create_function(|lua, (id, s): (String, Table)| put(lua, id, Kind::Box(vec3(s.get("at")?)?, vec3(s.get("size")?)?), colour(&s)?))?)?;
    t.set(
        "remove",
        lua.create_function(|lua, id: String| {
            super::api::state(lua).borrow_mut().shapes.retain(|x| x.id != id);
            Ok(())
        })?,
    )?;
    t.set(
        "clear",
        lua.create_function(|lua, ()| {
            super::api::state(lua).borrow_mut().shapes.clear();
            Ok(())
        })?,
    )?;
    t.set(
        "to_screen",
        lua.create_function(|lua, p: Value| {
            let p = vec3(p)?;
            let info = lua.app_data_ref::<std::rc::Rc<std::cell::RefCell<super::state::FrameInfo>>>().map(|f| f.clone()).unwrap_or_default();
            let f = info.borrow();
            let clip = f.view_proj * p.extend(1.0);
            if clip.w <= 1e-4 {
                return Ok(None);
            }
            let ndc = clip.truncate() / clip.w;
            let (x, y) = ((ndc.x + 1.0) / 2.0 * f.window[0], (1.0 - ndc.y) / 2.0 * f.window[1]);
            let out = lua.create_table()?;
            out.set("x", x.round() as i64)?;
            out.set("y", y.round() as i64)?;
            out.set("on_screen", ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0 && (0.0..=1.0).contains(&ndc.z))?;
            Ok(Some(out))
        })?,
    )?;
    g.set("draw", t)?;
    Ok(())
}

/// Every running mod's shapes as vertices: [line pairs, triangles].
pub(super) fn vertices(w: &World) -> [Vec<LineVertex>; 2] {
    let mut out: [Vec<LineVertex>; 2] = [Vec::new(), Vec::new()];
    for vm in w.lua.vms.iter().filter(|v| v.suspended.is_none()) {
        for s in vm.st.borrow().shapes.iter() {
            let color = s.colour.map(|c| c as f32 / 255.0);
            let v = |p: Vec3| LineVertex { pos: p.to_array(), color };
            match s.kind {
                Kind::Line(a, b) => out[0].extend([v(a), v(b)]),
                Kind::Ring(c, r, n, filled) => {
                    let at = |k: u32| {
                        let a = k as f32 / n as f32 * std::f32::consts::TAU;
                        c + Vec3::new(a.cos() * r, 0.0, a.sin() * r)
                    };
                    for k in 0..n {
                        if filled {
                            out[1].extend([v(c), v(at(k)), v(at(k + 1))]);
                        } else {
                            out[0].extend([v(at(k)), v(at(k + 1))]);
                        }
                    }
                }
                Kind::Text { .. } | Kind::Image { .. } => {}
                Kind::Box(c, size) => {
                    let h = size * 0.5;
                    let corner = |i: u32| c + Vec3::new(if i & 1 != 0 { h.x } else { -h.x }, if i & 2 != 0 { h.y } else { -h.y }, if i & 4 != 0 { h.z } else { -h.z });
                    for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
                        out[0].extend([v(corner(a)), v(corner(b))]);
                    }
                }
            }
        }
    }
    out
}

/// The texts and images in the world, with the HUD (after the name plates): projected each frame and depth-tested
/// against the scene; an image in 8 x 8 pieces so its perspective holds.
pub(super) fn draw_overlay(gu: &mut super::super::game_ui::GameUi, w: &World) {
    let (sw, sh) = w.ui_wh;
    let vp = w.lua.info.borrow().view_proj;
    let cam = super::super::camera::view(w);
    let project = |p: Vec3| -> Option<([f32; 2], f32)> {
        let clip = vp * p.extend(1.0);
        if clip.w <= 1e-4 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(([(ndc.x + 1.0) / 2.0 * sw, (1.0 - ndc.y) / 2.0 * sh], ndc.z))
    };
    for vm in w.lua.vms.iter().filter(|v| v.suspended.is_none()) {
        for s in vm.st.borrow().shapes.iter() {
            match &s.kind {
                Kind::Text { at, text, size, outline } => {
                    let Some(([x, y], z)) = project(*at) else { continue };
                    if !(0.0..=1.0).contains(&z) {
                        continue;
                    }
                    gu.depth(Some(z));
                    let tw = gu.width_in(text, None) * size;
                    let (x, y) = (x - tw / 2.0, y - 8.0 * size);
                    if let Some(o) = outline {
                        for (dx, dy) in [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0)] {
                            gu.text_in(text, x + dx, y + dy, super::hud::tint(*o), *size, None);
                        }
                    }
                    gu.text_in(text, x, y, super::hud::tint(s.colour), *size, None);
                    gu.depth(None);
                }
                Kind::Image { at, w: iw, h: ih, yaw, facing, key, img } => {
                    if !gu.has_image(key) {
                        gu.image(key, img.0, img.1, img.2.clone());
                    }
                    // The quad's across (u) and down (v) directions; y is down in the world.
                    let (fwd, right) = (Vec3::new(yaw.cos(), 0.0, -yaw.sin()), Vec3::new(yaw.sin(), 0.0, yaw.cos()));
                    let (u, v) = match facing {
                        Facing::Ground => (right, -fwd),
                        Facing::Upright => (right, Vec3::Y),
                        Facing::Camera => {
                            let inv = cam.view().inverse();
                            (inv.x_axis.truncate().normalize_or_zero(), -inv.y_axis.truncate().normalize_or_zero())
                        }
                    };
                    let point = |a: f32, b: f32| *at + u * (a - 0.5) * *iw + v * (b - 0.5) * *ih;
                    const N: usize = 8;
                    let colour = s.colour.map(|c| c as f32 / 255.0);
                    for j in 0..N {
                        for i in 0..N {
                            let (a0, a1, b0, b1) = (i as f32 / N as f32, (i + 1) as f32 / N as f32, j as f32 / N as f32, (j + 1) as f32 / N as f32);
                            let c = [point(a0, b0), point(a1, b0), point(a0, b1), point(a1, b1)].map(project);
                            let [Some(p0), Some(p1), Some(p2), Some(p3)] = c else { continue };
                            let z = (p0.1 + p1.1 + p2.1 + p3.1) / 4.0;
                            if !(0.0..=1.0).contains(&z) {
                                continue;
                            }
                            gu.depth(Some(z));
                            gu.image_corners(key, [p0.0, p1.0, p2.0, p3.0], [[a0, b0], [a1, b0], [a0, b1], [a1, b1]], colour);
                        }
                    }
                    gu.depth(None);
                }
                _ => {}
            }
        }
    }
}
