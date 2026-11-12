//! The mods' HUD layer (docs/lua-mods.md 4.10): retained text, rectangles, bars and icons placed in window pixels
//! (negative from the right or bottom edge), drawn under the windows in the active UI's fonts. Hidden with the UI.

use super::super::game_ui::HudFace;
use super::World;
use mlua::{Lua, Table, Value};

#[derive(Clone)]
pub(in crate::client) enum Kind {
    /// `font` None follows the UI; an outline (its colour and width) or a drop shadow under the text.
    Text {
        text: String,
        size: f32,
        colour: Option<[u8; 4]>,
        bg: Option<[u8; 4]>,
        align: u8,
        pad: f32,
        font: Option<HudFace>,
        outline: Option<[u8; 4]>,
        outline_w: f32,
        shadow: bool,
    },
    /// Filled, or outlined `line` wide; corners rounded `round` (the modern UI's).
    Rect {
        w: f32,
        h: f32,
        colour: [u8; 4],
        line: Option<f32>,
        round: f32,
    },
    Bar {
        w: f32,
        h: f32,
        value: f32,
        colour: Option<[u8; 4]>,
        kind: String,
    },
    Icon {
        icon: super::window::Icon,
        size: f32,
    },
    /// A PNG from the mod's folder: its texture key, pixels (w, h, RGBA), drawn size and tint.
    Image {
        key: String,
        img: std::sync::Arc<(u32, u32, Vec<u8>)>,
        w: f32,
        h: f32,
        tint: [u8; 4],
    },
    /// The zone's map (the one the client shows here): centred on world (x, z) or on us, `span` map pixels across, at
    /// `alpha` (none: the client option map_opacity, the Map Overlay mod's Map Overlay Opacity); our marker on it when
    /// `me`.
    Map {
        w: f32,
        h: f32,
        centre: Option<(f32, f32)>,
        span: f32,
        alpha: Option<f32>,
        me: bool,
    },
    /// An actor's face drawn live (client/portrait.rs): its server id, size, tint and finish.
    Portrait {
        entity: u32,
        w: f32,
        h: f32,
        tint: [u8; 4],
        look: Look,
    },
    /// A speech bubble: a rounded rect, its outline (colour, width) and a tail aimed at a point.
    Bubble {
        w: f32,
        h: f32,
        round: f32,
        colour: [u8; 4],
        edge: Option<[u8; 4]>,
        edge_w: f32,
        tail: Option<Tail>,
    },
}

/// A speech bubble's tail: the point it aims at (window pixels), its base width and its longest length.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(in crate::client) struct Tail {
    pub at: [f32; 2],
    pub w: f32,
    pub len: f32,
}

/// A bubble's outline (window pixels, clockwise on screen): the rounded rect with the tail let into the side facing
/// its point, and where the tail's three points are (base, tip, base).
fn bubble_outline(r: [f32; 4], round: f32, tail: Option<Tail>) -> (Vec<[f32; 2]>, Option<usize>) {
    let [x, y, w, h] = r;
    let rr = round.clamp(0.0, w.min(h) / 2.0);
    // The tail's side (0 top, 1 right, 2 bottom, 3 left), base centre and tip.
    let tail = tail.and_then(|t| {
        let [tx, ty] = t.at;
        let out = [y - ty, tx - (x + w), ty - (y + h), x - tx];
        let side = (0..4).max_by(|a, b| out[*a].total_cmp(&out[*b]))?;
        if out[side] <= 0.0 {
            return None;
        }
        let hw = (t.w / 2.0).max(1.0);
        let along = |lo: f32, len: f32, v: f32| {
            if len - 2.0 * (rr + hw) > 0.0 { v.clamp(lo + rr + hw, lo + len - rr - hw) } else { lo + len / 2.0 }
        };
        let (base, n) = match side {
            0 => ([along(x, w, tx), y], [0.0, -1.0]),
            1 => ([x + w, along(y, h, ty)], [1.0, 0.0]),
            2 => ([along(x, w, tx), y + h], [0.0, 1.0]),
            _ => ([x, along(y, h, ty)], [-1.0, 0.0]),
        };
        let (dx, dy) = (tx - base[0], ty - base[1]);
        let dist = (dx * dx + dy * dy).sqrt();
        let len = t.len.min(dist);
        if len < 2.0 {
            return None;
        }
        // Kept from lying along its side: at least half of it straight out.
        let (mut ux, mut uy) = (dx / dist, dy / dist);
        let o = ux * n[0] + uy * n[1];
        if o < 0.5 {
            let (px, py) = (ux - n[0] * o, uy - n[1] * o);
            let pl = (px * px + py * py).sqrt().max(1e-6);
            (ux, uy) = (n[0] * 0.5 + px / pl * 0.866, n[1] * 0.5 + py / pl * 0.866);
        }
        Some((side, base, hw, [base[0] + ux * len, base[1] + uy * len]))
    });
    let steps = ((rr / 2.0).ceil() as usize).clamp(1, 12);
    let corners = [
        ([x + rr, y + rr], 1.0f32),
        ([x + w - rr, y + rr], 1.5),
        ([x + w - rr, y + h - rr], 0.0),
        ([x + rr, y + h - rr], 0.5),
    ];
    // The direction each side runs in, after its corner: top right, right down, bottom left, left up.
    let runs = [[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0]];
    let mut pts: Vec<[f32; 2]> = Vec::new();
    let mut tip = None;
    let push = |pts: &mut Vec<[f32; 2]>, p: [f32; 2]| {
        if pts.last().is_none_or(|q: &[f32; 2]| (q[0] - p[0]).abs() + (q[1] - p[1]).abs() > 0.01) {
            pts.push(p);
        }
    };
    for (k, (c, a0)) in corners.iter().enumerate() {
        for s in 0..=steps {
            let a = (a0 + 0.5 * s as f32 / steps as f32) * std::f32::consts::PI;
            push(&mut pts, [c[0] + rr * a.cos(), c[1] + rr * a.sin()]);
        }
        if let Some((side, b, hw, t)) = tail
            && side == k
        {
            let d = runs[k];
            push(&mut pts, [b[0] - d[0] * hw, b[1] - d[1] * hw]);
            tip = Some(pts.len());
            pts.push(t);
            push(&mut pts, [b[0] + d[0] * hw, b[1] + d[1] * hw]);
        }
    }
    if pts.len() > 1 && (pts[0][0] - pts[pts.len() - 1][0]).abs() + (pts[0][1] - pts[pts.len() - 1][1]).abs() <= 0.01 {
        pts.pop();
    }
    (pts, tip)
}

/// A triangle's corners, each with its straight colour (0..1).
pub(in crate::client) type Tri = [([f32; 2], [f32; 4]); 3];

/// A speech bubble as triangles in window pixels, each corner with its straight colour (0..1): the fill, an outline
/// `edge` wide centred on the outline, and a pixel of feathering outside it.
pub(in crate::client) fn bubble_tris(
    r: [f32; 4],
    round: f32,
    tail: Option<Tail>,
    fill: [u8; 4],
    edge: Option<([u8; 4], f32)>,
) -> Vec<Tri> {
    let (pts, tip) = bubble_outline(r, round, tail);
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let rgba =
        |c: [u8; 4], a: f32| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, c[3] as f32 / 255.0 * a];
    // Each point's offset direction outward, mitred (outward of a segment d is (d.y, -d.x) going clockwise).
    let out = |d: [f32; 2]| {
        let l = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-6);
        [d[1] / l, -d[0] / l]
    };
    let off: Vec<[f32; 2]> = (0..n)
        .map(|i| {
            let (p, c, q) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let (n1, n2) = (out([c[0] - p[0], c[1] - p[1]]), out([q[0] - c[0], q[1] - c[1]]));
            let (sx, sy) = (n1[0] + n2[0], n1[1] + n2[1]);
            let l = (sx * sx + sy * sy).sqrt();
            if l < 1e-4 {
                return n1;
            }
            let m = [sx / l, sy / l];
            let k = 1.0 / (m[0] * n1[0] + m[1] * n1[1]).max(0.25);
            [m[0] * k, m[1] * k]
        })
        .collect();
    let at = |i: usize, d: f32| [pts[i][0] + off[i][0] * d, pts[i][1] + off[i][1] * d];
    let half = edge.map_or(0.0, |e| e.1.max(0.0) / 2.0);
    let inner: Vec<[f32; 2]> = (0..n).map(|i| at(i, -half)).collect();
    let mut tris = Vec::new();
    let f = rgba(fill, 1.0);
    let c = [r[0] + r[2] / 2.0, r[1] + r[3] / 2.0];
    let ring: Vec<usize> = (0..n).filter(|i| Some(*i) != tip).collect();
    for k in 0..ring.len() {
        let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
        tris.push([(c, f), (inner[a], f), (inner[b], f)]);
    }
    if let Some(t) = tip {
        tris.push([(inner[t - 1], f), (inner[t], f), (inner[(t + 1) % n], f)]);
    }
    let quad = |tris: &mut Vec<Tri>, a: [([f32; 2], [f32; 4]); 2], b: [([f32; 2], [f32; 4]); 2]| {
        tris.push([a[0], a[1], b[0]]);
        tris.push([a[1], b[1], b[0]]);
    };
    let (rim, col) = match edge.filter(|e| e.1 > 0.0) {
        Some((e, _)) => {
            let ec = rgba(e, 1.0);
            for i in 0..n {
                let j = (i + 1) % n;
                quad(&mut tris, [(inner[i], ec), (at(i, half), ec)], [(inner[j], ec), (at(j, half), ec)]);
            }
            (half, e)
        }
        None => (0.0, fill),
    };
    let (solid, clear) = (rgba(col, 1.0), rgba(col, 0.0));
    for i in 0..n {
        let j = (i + 1) % n;
        quad(
            &mut tris,
            [(at(i, rim), solid), (at(i, rim + 1.0), clear)],
            [(at(j, rim), solid), (at(j, rim + 1.0), clear)],
        );
    }
    tris
}

/// A portrait's finish, drawn into its picture: a backing, an oval cut and a frame (its width in window pixels).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(in crate::client) struct Look {
    pub bg: Option<[u8; 4]>,
    pub oval: bool,
    pub frame: Option<[u8; 4]>,
    pub frame_w: f32,
}

/// What a portrait element asks of client/portrait.rs: its actor, shown size in window pixels and finish.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(in crate::client) struct Wanted {
    pub entity: u32,
    pub size: [f32; 2],
    pub look: Look,
}

#[derive(Clone)]
pub(in crate::client) struct Elem {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub visible: bool,
    pub layer: i32,
    pub kind: Kind,
    /// The tooltip shown while the pointer rests on it: (title, text).
    pub tip: Option<(String, String)>,
    /// A modern panel its place counts from (ui.native.panels names them), and which corner: bit 0 right, bit 1
    /// bottom.
    pub anchor: Option<String>,
    pub corner: u8,
    /// The HUD window it belongs to (its opacity: hud.opacity); "hud" by default.
    pub window: String,
}

pub(super) fn colour(v: Value) -> mlua::Result<Option<[u8; 4]>> {
    Ok(match v {
        Value::Table(t) => {
            let get = |k: &str, i: i64, d: u8| -> mlua::Result<u8> {
                Ok(t.get::<Option<u8>>(k)?.or(t.get::<Option<u8>>(i)?).unwrap_or(d))
            };
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
    // A panel's name, or false for the window.
    if t.contains_key("anchor")? {
        e.anchor = match t.get::<Value>("anchor")? {
            Value::String(s) => {
                let s = s.to_string_lossy();
                if !super::native::PANELS.iter().any(|p| p.name == s) {
                    return Err(mlua::Error::runtime(format!(
                        "no panel {s:?} to anchor to (ui.native.panels names them)"
                    )));
                }
                Some(s)
            }
            _ => None,
        };
    }
    if let Some(v) = t.get::<Option<String>>("window")? {
        e.window = v;
    }
    if let Some(c) = t.get::<Option<String>>("corner")? {
        e.corner = match c.as_str() {
            "top_left" => 0,
            "top_right" => 1,
            "bottom_left" => 2,
            "bottom_right" => 3,
            _ => {
                return Err(mlua::Error::runtime(format!(
                    "corner {c:?}: top_left, top_right, bottom_left or bottom_right"
                )));
            }
        };
    }
    // A string, {title, text}, or false for none.
    if t.contains_key("tip")? {
        e.tip = match t.get::<Value>("tip")? {
            Value::String(s) => Some((String::new(), s.to_string_lossy())),
            Value::Table(tt) => Some((
                tt.get::<Option<String>>("title")?.unwrap_or_default(),
                tt.get::<Option<String>>("text")?.unwrap_or_default(),
            )),
            _ => None,
        };
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
                    // Any other name follows the UI's face.
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
        Kind::Rect { w, h, colour: c, line, round } => {
            if let Some(v) = t.get::<Option<f32>>("round")? {
                *round = v.max(0.0);
            }
            if let Some(v) = t.get::<Option<f32>>("w")? {
                *w = v;
            }
            if let Some(v) = t.get::<Option<f32>>("h")? {
                *h = v;
            }
            if let Some(v) = colour(t.get("colour")?)? {
                *c = v;
            }
            // A width, or false for filled.
            if t.contains_key("line")? {
                *line = t.get::<Option<f32>>("line").ok().flatten().map(|v| v.clamp(0.5, 32.0));
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
        Kind::Map { w, h, centre, span, alpha, me } => {
            if let Some(v) = t.get::<Option<bool>>("me")? {
                *me = v;
            }
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
            // A value, or false for the client option map_opacity (the Map Overlay mod's Map Overlay Opacity).
            if t.contains_key("alpha")? {
                *alpha = t.get::<Option<f32>>("alpha").ok().flatten().map(|v| v.clamp(0.0, 1.0));
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
        Kind::Portrait { entity, w, h, tint, look } => {
            if let Some(v) = t.get::<Option<u32>>("entity")? {
                *entity = v;
            }
            if let Some(v) = t.get::<Option<f32>>("size")? {
                (*w, *h) = (v.max(1.0), v.max(1.0));
            }
            if let Some(v) = t.get::<Option<f32>>("w")? {
                *w = v.max(1.0);
            }
            if let Some(v) = t.get::<Option<f32>>("h")? {
                *h = v.max(1.0);
            }
            if let Some(v) = colour(t.get("colour")?)? {
                *tint = v;
            }
            if t.contains_key("bg")? {
                look.bg = colour(t.get("bg")?)?;
            }
            if t.contains_key("frame")? {
                look.frame = colour(t.get("frame")?)?;
            }
            if let Some(v) = t.get::<Option<f32>>("frame_w")? {
                look.frame_w = v.max(0.0);
            }
            if let Some(v) = t.get::<Option<String>>("shape")? {
                look.oval = match v.as_str() {
                    "oval" => true,
                    "square" => false,
                    _ => return Err(mlua::Error::runtime(format!("shape {v:?}: oval or square"))),
                };
            }
        }
        Kind::Bubble { w, h, round, colour: c, edge, edge_w, tail } => {
            if let Some(v) = t.get::<Option<f32>>("w")? {
                *w = v.max(1.0);
            }
            if let Some(v) = t.get::<Option<f32>>("h")? {
                *h = v.max(1.0);
            }
            if let Some(v) = t.get::<Option<f32>>("round")? {
                *round = v.max(0.0);
            }
            if let Some(v) = colour(t.get("colour")?)? {
                *c = v;
            }
            if t.contains_key("edge")? {
                *edge = colour(t.get("edge")?)?;
            }
            if let Some(v) = t.get::<Option<f32>>("edge_w")? {
                *edge_w = v.max(0.0);
            }
            // A point {x, y} (window pixels) the tail aims at, or false for none; its base width and longest length.
            if t.contains_key("tail")? {
                *tail = match t.get::<Value>("tail")? {
                    Value::Table(p) => Some(Tail {
                        at: [p.get("x")?, p.get("y")?],
                        w: tail.map_or(18.0, |t| t.w),
                        len: tail.map_or(22.0, |t| t.len),
                    }),
                    Value::Nil | Value::Boolean(false) => None,
                    _ => return Err(mlua::Error::runtime("tail is a point {x, y}, or false for none")),
                };
            }
            if let Some(tl) = tail.as_mut() {
                if let Some(v) = t.get::<Option<f32>>("tail_w")? {
                    tl.w = v.max(1.0);
                }
                if let Some(v) = t.get::<Option<f32>>("tail_len")? {
                    tl.len = v.max(0.0);
                }
            }
        }
        Kind::Icon { icon, size } => {
            // As list and canvas icons: kind "item" (the default), "status", "spell" or "ability".
            let kind = t.get::<Option<String>>("kind")?;
            let id = t.get::<Option<u16>>("id")?.unwrap_or(icon.id());
            *icon = super::window::Icon::of(Some(kind.as_deref().unwrap_or(icon.kind())), id);
            if let Some(v) = t.get::<Option<f32>>("size")? {
                *size = v.max(1.0);
            }
        }
    }
    Ok(())
}

fn put(lua: &Lua, id: String, kind: Kind, spec: Table) -> mlua::Result<()> {
    let mut e = Elem {
        id: id.clone(),
        x: 0.0,
        y: 0.0,
        visible: true,
        layer: 0,
        kind,
        tip: None,
        anchor: None,
        corner: 0,
        window: "hud".into(),
    };
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
pub(super) fn load_png(
    src: &super::vm::Source,
    mod_id: &str,
    path: &str,
) -> mlua::Result<(String, std::sync::Arc<(u32, u32, Vec<u8>)>)> {
    if super::vm::outside(path) {
        return Err(mlua::Error::runtime(format!("{path:?}: outside the mod's folder")));
    }
    let bytes = match src {
        super::vm::Source::Dir(d) => super::vm::inside(d, path)
            .and_then(std::fs::read)
            .map_err(|e| mlua::Error::runtime(format!("{path}: {e}")))?,
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
            if super::vm::outside(&path) {
                return Err(mlua::Error::runtime(format!("{path:?}: outside the mod's folder")));
            }
            let bytes = match &src2 {
                super::vm::Source::Dir(d) => super::vm::inside(d, &path)
                    .and_then(std::fs::read)
                    .map_err(|e| mlua::Error::runtime(format!("{path}: {e}")))?,
                super::vm::Source::Bundled(_) => return Err(mlua::Error::runtime("bundled mods carry no image files")),
            };
            let (w, h, rgba) = decode_png(&bytes).map_err(|e| mlua::Error::runtime(format!("{path}: {e} (a PNG)")))?;
            let c = super::super::chaticons::add(&format!("{mod2}/{path}"), w, h, rgba)
                .ok_or_else(|| mlua::Error::runtime("too many chat icons"))?;
            Ok(c.to_string())
        })?,
    )?;
    t.set(
        "text",
        lua.create_function(|lua, (id, spec): (String, Table)| {
            put(
                lua,
                id,
                Kind::Text {
                    text: String::new(),
                    size: 1.0,
                    colour: None,
                    bg: None,
                    align: 0,
                    pad: 4.0,
                    font: None,
                    outline: None,
                    outline_w: 1.0,
                    shadow: false,
                },
                spec,
            )
        })?,
    )?;
    t.set(
        "rect",
        lua.create_function(|lua, (id, spec): (String, Table)| {
            put(lua, id, Kind::Rect { w: 100.0, h: 20.0, colour: [0, 0, 0, 160], line: None, round: 0.0 }, spec)
        })?,
    )?;
    t.set(
        "bar",
        lua.create_function(|lua, (id, spec): (String, Table)| {
            put(lua, id, Kind::Bar { w: 120.0, h: 8.0, value: 0.0, colour: None, kind: "hp".into() }, spec)
        })?,
    )?;
    t.set(
        "icon",
        lua.create_function(|lua, (id, spec): (String, Table)| {
            put(lua, id, Kind::Icon { icon: super::window::Icon::Item(0), size: 32.0 }, spec)
        })?,
    )?;
    t.set(
        "map",
        lua.create_function(|lua, (id, spec): (String, Table)| {
            put(lua, id, Kind::Map { w: 200.0, h: 200.0, centre: None, span: 256.0, alpha: None, me: false }, spec)
        })?,
    )?;
    t.set(
        "bubble",
        lua.create_function(|lua, (id, spec): (String, Table)| {
            put(
                lua,
                id,
                Kind::Bubble {
                    w: 200.0,
                    h: 60.0,
                    round: 10.0,
                    colour: [14, 16, 26, 235],
                    edge: None,
                    edge_w: 2.0,
                    tail: None,
                },
                spec,
            )
        })?,
    )?;
    t.set(
        "portrait",
        lua.create_function(|lua, (id, spec): (String, Table)| {
            if spec.get::<Option<u32>>("entity")?.is_none() {
                return Err(mlua::Error::runtime("a portrait needs entity (a server id)"));
            }
            put(
                lua,
                id,
                Kind::Portrait {
                    entity: 0,
                    w: 96.0,
                    h: 96.0,
                    tint: [255; 4],
                    look: Look { bg: None, oval: true, frame: None, frame_w: 3.0 },
                },
                spec,
            )
        })?,
    )?;
    let (src3, mod3) = (vm.src.clone(), vm.manifest.id.clone());
    t.set(
        "set",
        lua.create_function(move |lua, (id, fields): (String, Table)| {
            let fields = super::api::unhooked(lua, &fields)?;
            let png = fields.get::<Option<String>>("path")?.map(|p| load_png(&src3, &mod3, &p)).transpose()?;
            let st = super::api::state(lua);
            let mut s = st.borrow_mut();
            let e = s
                .hud
                .iter_mut()
                .find(|x| x.id == id)
                .ok_or_else(|| mlua::Error::runtime(format!("no HUD element {id:?}")))?;
            // A new picture: its size unless the fields give one.
            if let Some((k, i)) = png {
                let Kind::Image { key, img, w, h, .. } = &mut e.kind else {
                    return Err(mlua::Error::runtime("path is an image's field"));
                };
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
        "opacity",
        lua.create_function(|lua, (window, opacity): (String, Option<f32>)| {
            super::api::set_opacity(lua, &format!("hud:{}:{window}", super::api::mod_id(lua)?), opacity);
            Ok(())
        })?,
    )?;
    t.set(
        "screen",
        lua.create_function(|lua, ()| {
            let info = lua
                .app_data_ref::<std::rc::Rc<std::cell::RefCell<super::state::FrameInfo>>>()
                .map(|f| f.clone())
                .unwrap_or_default();
            let f = info.borrow();
            let out = lua.create_table()?;
            out.set("width", f.window[0] as i64)?;
            out.set("height", f.window[1] as i64)?;
            // The modern UI's point, else the classic layout's unit.
            out.set("ui_scale", f.modern_ppp.unwrap_or(if f.k.0 > 0.0 { 1.0 / f.k.0 } else { 1.0 }))?;
            Ok(out)
        })?,
    )?;
    t.set(
        "bounds",
        lua.create_function(|lua, id: String| {
            let info = lua
                .app_data_ref::<std::rc::Rc<std::cell::RefCell<super::state::FrameInfo>>>()
                .map(|f| f.clone())
                .unwrap_or_default();
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
/// Every running mod's visible elements by layer, then load order: (mod, element).
pub(in crate::client) fn elements(host: &super::Host) -> Vec<(String, Elem)> {
    let mut all: Vec<(String, Elem)> = Vec::new();
    for vm in host.vms.iter().filter(|v| v.suspended.is_none()) {
        for e in vm.st.borrow().hud.iter().filter(|e| e.visible) {
            all.push((vm.manifest.id.clone(), e.clone()));
        }
    }
    all.sort_by_key(|(_, e)| e.layer);
    all
}

/// An element's place in window pixels. Anchored: signed offsets from a corner of the panel (as last drawn, so where a
/// hidden panel was; none before a panel ever was). Else negative places count from the window's right or bottom edge.
pub(in crate::client) fn place(
    e: &Elem,
    panels: &std::collections::BTreeMap<String, ([f32; 4], bool)>,
    win: [f32; 2],
) -> Option<(f32, f32)> {
    Some(match &e.anchor {
        Some(a) => {
            let (r, _) = panels.get(a)?;
            (
                r[0] + if e.corner & 1 != 0 { r[2] } else { 0.0 } + e.x,
                r[1] + if e.corner & 2 != 0 { r[3] } else { 0.0 } + e.y,
            )
        }
        None => (if e.x < 0.0 { win[0] + e.x } else { e.x }, if e.y < 0.0 { win[1] + e.y } else { e.y }),
    })
}

/// Whether the game UI draws an element in the modern UI: only text in the game's font (the modern UI's painter has
/// it not); the rest is the modern layer's (modern/hudmods.rs), drawn as its panels are.
fn game_drawn(e: &Elem) -> bool {
    matches!(e.kind, Kind::Text { font: Some(HudFace::Game), .. })
}

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
    let mut rects = Vec::new();
    // Panels are the modern UI's: in the classic UI anchored elements wait.
    let panels = if w.modern { info.borrow().native_rects.clone() } else { Default::default() };
    for (mod_id, e) in elements(host).into_iter().filter(|(_, e)| !w.modern || game_drawn(e)) {
        let Some((px, py)) = place(&e, &panels, win) else { continue };
        // Its HUD window's opacity, over everything it draws.
        let o = host.window_opacity(&w.window_opacity, &format!("hud:{mod_id}:{}", e.window));
        gu.opacity((o < 1.0).then_some(o));
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
                        for (dx, dy) in [
                            (-1.0, -1.0),
                            (0.0, -1.0),
                            (1.0, -1.0),
                            (-1.0, 0.0),
                            (1.0, 0.0),
                            (-1.0, 1.0),
                            (0.0, 1.0),
                            (1.0, 1.0),
                        ] {
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
            Kind::Rect { w: rw, h: rh, colour, line, .. } => {
                let (rw, rh) = (rw * k.0, rh * k.1);
                match line {
                    Some(l) => {
                        let (lx, ly) = (l * k.0, l * k.1);
                        for r in [
                            [x, y, rw, ly],
                            [x, y + rh - ly, rw, ly],
                            [x, y + ly, lx, rh - 2.0 * ly],
                            [x + rw - lx, y + ly, lx, rh - 2.0 * ly],
                        ] {
                            gu.rect(r[0], r[1], r[2], r[3], rgba(*colour));
                        }
                    }
                    None => gu.rect(x, y, rw, rh, rgba(*colour)),
                }
                [x, y, rw, rh]
            }
            Kind::Bar { w: bw, h: bh, value, colour, kind } => {
                let (bw, bh) = (bw * k.0, bh * k.1);
                gu.rect(x, y, bw, bh, [0.0, 0.0, 0.0, 0.55]);
                if *value > 0.0 {
                    gu.rect(
                        x + 1.0,
                        y + 1.0,
                        (bw - 2.0).max(0.0) * value,
                        (bh - 2.0).max(0.0),
                        rgba(colour.unwrap_or_else(|| bar_colour(kind))),
                    );
                }
                [x, y, bw, bh]
            }
            Kind::Map { w: mw, h: mh, centre, span, alpha, me } => {
                let (mw, mh) = (mw * k.0, mh * k.1);
                let regions = gu.map_regions().to_vec();
                let mut m = w.lua.map.borrow_mut();
                match m.as_mut() {
                    Some(o) if o.zone == w.pos_zone => o.follow(w, &regions),
                    _ => *m = Some(super::super::map::Overlay::open_hud(w, &regions)),
                }
                if let Some(o) = m.as_mut() {
                    let facing = w.net.me.lock().map_or(0.0, |m| super::super::event::rad_of(m.dir));
                    let marker = me.then_some(((w.pos.x, w.pos.z), facing));
                    o.draw_window(
                        gu,
                        [x, y, mw, mh],
                        centre.unwrap_or((w.pos.x, w.pos.z)),
                        *span,
                        alpha.unwrap_or(w.map_opacity),
                        marker,
                    );
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
            Kind::Icon { icon, size } => {
                let s = size * k.0;
                super::window::icon(gu, Some(w), *icon, x, y, s, [255; 4]);
                [x, y, s, s]
            }
            Kind::Bubble { w: bw, h: bh, round, colour, edge, edge_w, tail } => {
                for tri in bubble_tris([px, py, *bw, *bh], *round, *tail, *colour, edge.map(|e| (e, *edge_w))) {
                    let p = tri.map(|(q, _)| [q[0] * k.0, q[1] * k.1]);
                    let c = tri.map(|(_, c)| c);
                    gu.quad([p[0], p[1], p[2], p[2]], [c[0], c[1], c[2], c[2]]);
                }
                [x, y, bw * k.0, bh * k.1]
            }
            // Nothing until the actor has been drawn into one (none while it is not posed).
            Kind::Portrait { entity, w: iw, h: ih, tint, .. } => {
                let (pw, ph) = (iw * k.0, ih * k.1);
                if let Some((key, _)) = host.portrait(*entity) {
                    gu.image_rect(&key, x, y, pw, ph, tint.map(|c| c as f32 / 255.0));
                }
                [x, y, pw, ph]
            }
        };
        rects.push((mod_id, e.id.clone(), [r[0] / k.0, r[1] / k.1, r[2] / k.0, r[3] / k.1]));
    }
    gu.opacity(None);
    info.borrow_mut().hud_rects = rects;
    // The classic UI's tooltip (the modern UI's is the kit's, modern/mod.rs), once the pointer rests a moment.
    if !w.modern
        && let Some((title, text, secs)) = tip(host)
        && secs >= 0.35
    {
        let (cx, cy) = host.input.borrow().cursor;
        draw_tip(gu, &title, &text, (cx as f32 * k.0, cy as f32 * k.1), (win[0] * k.0, win[1] * k.1));
    }
}

/// A tooltip box by the pointer (UI units), kept on screen: the title in gold over the text.
fn draw_tip(gu: &mut super::super::game_ui::GameUi, title: &str, text: &str, at: (f32, f32), screen: (f32, f32)) {
    let lines: Vec<(&str, [u8; 4])> = std::iter::once(title)
        .filter(|t| !t.is_empty())
        .map(|t| (t, [255, 210, 120, 255]))
        .chain(text.split('\n').map(|l| (l, [255, 255, 255, 255])))
        .collect();
    let pad = 6.0;
    let lw = lines.iter().map(|(l, _)| gu.width_in(l, None)).fold(0.0, f32::max);
    let (bw, bh) = (lw + pad * 2.0, 16.0 * lines.len() as f32 + pad * 2.0);
    let x = (at.0 + 16.0).min(screen.0 - bw).max(0.0);
    let y = if at.1 + 24.0 + bh > screen.1 { at.1 - bh - 4.0 } else { at.1 + 24.0 };
    gu.rect(x, y, bw, bh, [0.04, 0.06, 0.09, 0.92]);
    for (n, (l, c)) in lines.iter().enumerate() {
        gu.text_in(l, x + pad, y + pad + 16.0 * n as f32, tint(*c), 1.0, None);
    }
}

fn inside(r: &[f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
}

/// The topmost element with a handler (or, with `tips`, a tooltip) under a window pixel: (mod, element).
fn under(host: &super::Host, x: f64, y: f64, tips: bool) -> Option<(String, String)> {
    let f = host.info.borrow();
    let (x, y) = (x as f32, y as f32);
    let wanted = |m: &str, id: &str| {
        host.vms.iter().any(|v| {
            if v.manifest.id != m || v.suspended.is_some() {
                return false;
            }
            let st = v.st.borrow();
            st.hud_handlers.iter().any(|h| h.0 == id) || (tips && st.hud.iter().any(|e| e.id == id && e.tip.is_some()))
        })
    };
    f.hud_rects
        .iter()
        .rev()
        .find(|(m, id, r)| inside(r, x, y) && wanted(m, id))
        .map(|(m, id, _)| (m.clone(), id.clone()))
}

/// The topmost element with a handler under a window pixel: (mod, element).
pub(super) fn hit(host: &super::Host, x: f64, y: f64) -> Option<(String, String)> {
    under(host, x, y, false)
}

/// Mod `m`'s topmost element with a handler under a window pixel, other than `dragged` (where a drag ends).
pub(super) fn drop_target(host: &super::Host, m: &str, dragged: &str, x: f64, y: f64) -> Option<String> {
    let f = host.info.borrow();
    let vm = host.vms.iter().find(|v| v.manifest.id == m)?;
    let st = vm.st.borrow();
    f.hud_rects
        .iter()
        .rev()
        .find(|(mm, id, r)| {
            mm == m && id != dragged && inside(r, x as f32, y as f32) && st.hud_handlers.iter().any(|h| h.0 == *id)
        })
        .map(|(_, id, _)| id.clone())
}

/// The element under the pointer (one with a handler or a tooltip): mouse_leave and mouse_enter as it changes.
/// None under a modern panel.
pub(super) fn hover(host: &mut super::Host) {
    let (x, y) = host.input.borrow().cursor;
    let now = if host.input.borrow().over_ui { None } else { under(host, x, y, true) };
    let mut s = host.input.borrow_mut();
    let was = s.hud_hover.as_ref().map(|h| (h.0.clone(), h.1.clone()));
    if was == now {
        return;
    }
    let e = |kind: &str| serde_json::json!({"kind": kind, "x": x.round() as i64, "y": y.round() as i64});
    if let Some((m, id)) = was {
        host.hud_events.push((m, id, e("mouse_leave")));
    }
    if let Some((m, id)) = now.clone() {
        host.hud_events.push((m, id, e("mouse_enter")));
    }
    s.hud_hover = now.map(|(m, id)| (m, id, std::time::Instant::now()));
}

/// The tooltip of the element under the pointer, and how long it has been there.
pub(super) fn tip(host: &super::Host) -> Option<(String, String, f32)> {
    let s = host.input.borrow();
    let (m, id, since) = s.hud_hover.as_ref()?;
    let vm = host.vms.iter().find(|v| v.manifest.id == *m && v.suspended.is_none())?;
    let (title, text) = vm.st.borrow().hud.iter().find(|e| e.id == *id && e.visible)?.tip.clone()?;
    Some((title, text, since.elapsed().as_secs_f32()))
}
