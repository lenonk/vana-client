//! Render overrides for mods (docs/lua-mods.md 4.11): how an entity draws (hidden, name, name colour, alpha, tint,
//! size) and the camera's distance. Looks last while the mod is loaded; the newest mod's wins per entity.

use super::api::world;
use mlua::{Lua, Table, Value};

/// One entity's drawn look as a mod set it.
#[derive(Clone, Default)]
pub(in crate::client) struct Look {
    pub hidden: bool,
    pub name: Option<String>,
    /// RGBA, 0..255.
    pub name_colour: Option<[u8; 4]>,
    /// 0..1, at most the model's.
    pub alpha: Option<f32>,
    /// RGB multipliers.
    pub tint: Option<[f32; 3]>,
    pub scale: Option<f32>,
}

impl Look {
    /// The plate's colour as the game's (ARGB, 0x80 = 1.0).
    pub fn plate_tint(&self) -> Option<u32> {
        let h = |v: u8| u8::try_from(u16::from(v).div_ceil(2)).unwrap_or(0x80);
        self.name_colour.map(|c| u32::from_be_bytes([h(c[3]), h(c[0]), h(c[1]), h(c[2])]))
    }
}

/// The parts of a player-style look a mod sets (render.look); the others stay the entity's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(in crate::client) struct Model {
    pub race: Option<u8>,
    pub face: Option<u16>,
    /// head, body, hands, legs, feet, main, sub, range (model ids).
    pub parts: [Option<u16>; 8],
}

/// The look's slot names in GrapIDTbl order after the face.
pub(super) const PARTS: [&str; 8] = ["head", "body", "hands", "legs", "feet", "main", "sub", "range"];

impl Model {
    pub fn applied(&self, l: crate::dat::character::Look) -> crate::dat::character::Look {
        let p = |k: usize, v: u16| self.parts[k].unwrap_or(v);
        crate::dat::character::Look {
            race: self.race.unwrap_or(l.race),
            face: self.face.unwrap_or(l.face),
            head: p(0, l.head),
            body: p(1, l.body),
            hands: p(2, l.hands),
            legs: p(3, l.legs),
            feet: p(4, l.feet),
            main: p(5, l.main),
            sub: p(6, l.sub),
            range: p(7, l.range),
        }
    }
}

fn info(lua: &Lua) -> std::rc::Rc<std::cell::RefCell<super::state::FrameInfo>> {
    lua.app_data_ref::<std::rc::Rc<std::cell::RefCell<super::state::FrameInfo>>>()
        .map(|f| f.clone())
        .unwrap_or_default()
}

pub(super) fn colour(v: Value) -> mlua::Result<Option<[u8; 4]>> {
    Ok(match v {
        Value::Table(t) => {
            let get = |k: &str, i: i64, d: u8| -> mlua::Result<u8> {
                Ok(t.get::<Option<u8>>(k)?.or(t.get::<Option<u8>>(i)?).unwrap_or(d))
            };
            Some([get("r", 1, 255)?, get("g", 2, 255)?, get("b", 3, 255)?, get("a", 4, 255)?])
        }
        _ => None,
    })
}

pub(super) fn install(lua: &Lua, g: &Table, mod_id: &str) -> mlua::Result<()> {
    let t = lua.create_table()?;
    let me = mod_id.to_string();
    t.set(
        "entity",
        lua.create_function(move |lua, (id, spec): (u32, Option<Table>)| {
            let spec = spec.map(|s| super::api::unhooked(lua, &s)).transpose()?;
            let info = info(lua);
            let mut f = info.borrow_mut();
            let Some(s) = spec else {
                if f.looks.get(&id).is_some_and(|l| l.0 == me) {
                    f.looks.remove(&id);
                }
                return Ok(());
            };
            let tint = match s.get::<Value>("tint")? {
                Value::Table(c) => {
                    let get = |k: &str, i: i64| -> mlua::Result<f32> {
                        Ok(c.get::<Option<f32>>(k)?.or(c.get::<Option<f32>>(i)?).unwrap_or(1.0))
                    };
                    Some([get("r", 1)?, get("g", 2)?, get("b", 3)?])
                }
                _ => None,
            };
            let look = Look {
                hidden: s.get::<Option<bool>>("hidden")?.unwrap_or(false),
                name: s.get::<Option<String>>("name")?,
                name_colour: colour(s.get("name_colour")?)?,
                alpha: s.get::<Option<f32>>("alpha")?.map(|a| a.clamp(0.0, 1.0)),
                tint,
                scale: s.get::<Option<f32>>("scale")?.map(|k| k.clamp(0.05, 20.0)),
            };
            f.looks.insert(id, (me.clone(), look));
            Ok(())
        })?,
    )?;
    let me = mod_id.to_string();
    t.set(
        "look",
        lua.create_function(move |lua, (id, spec): (u32, Option<Table>)| {
            let spec = spec.map(|s| super::api::unhooked(lua, &s)).transpose()?;
            let info = info(lua);
            let mut f = info.borrow_mut();
            let Some(s) = spec else {
                if f.models.get(&id).is_some_and(|m| m.0 == me) {
                    f.models.remove(&id);
                    f.models_ver += 1;
                }
                return Ok(());
            };
            let mut m =
                Model { race: s.get::<Option<u8>>("race")?, face: s.get::<Option<u16>>("face")?, ..Model::default() };
            if m.race.is_some_and(|r| !(1..=8).contains(&r)) {
                return Err(mlua::Error::runtime("render.look: race is 1..8"));
            }
            if m.face.is_some_and(|f| f > 0xff) {
                return Err(mlua::Error::runtime("render.look: face is 0..255"));
            }
            for (k, name) in PARTS.iter().enumerate() {
                let v = s.get::<Option<u16>>(*name)?;
                if v.is_some_and(|v| v > 0xfff) {
                    return Err(mlua::Error::runtime(format!("render.look: {name} is a model id 0..4095")));
                }
                m.parts[k] = v;
            }
            if f.models.get(&id) != Some(&(me.clone(), m)) {
                f.models.insert(id, (me.clone(), m));
                f.models_ver += 1;
            }
            Ok(())
        })?,
    )?;
    let me = mod_id.to_string();
    t.set(
        "clear",
        lua.create_function(move |lua, ()| {
            let info = info(lua);
            let mut f = info.borrow_mut();
            f.looks.retain(|_, l| l.0 != me);
            let n = f.models.len();
            f.models.retain(|_, m| m.0 != me);
            if f.models.len() != n {
                f.models_ver += 1;
            }
            Ok(())
        })?,
    )?;
    let me = mod_id.to_string();
    t.set(
        "shader",
        lua.create_function(move |lua, (id, src): (String, Option<String>)| {
            let info = info(lua);
            let mut f = info.borrow_mut();
            let at = f.shaders.iter().position(|s| s.0 == me && s.1 == id);
            match (src, at) {
                (Some(src), at) => {
                    f.shader_ver += 1;
                    let params = at.map_or([[0.0; 4]; 4], |k| f.shaders[k].4);
                    let pass = (me.clone(), id, f.shader_ver, std::sync::Arc::<str>::from(src), params);
                    match at {
                        Some(k) => f.shaders[k] = pass,
                        None => f.shaders.push(pass),
                    }
                }
                (None, Some(k)) => {
                    f.shaders.remove(k);
                }
                (None, None) => {}
            }
            Ok(())
        })?,
    )?;
    let me = mod_id.to_string();
    t.set(
        "shader_params",
        lua.create_function(move |lua, (id, params): (String, Vec<Table>)| {
            let params = params.iter().map(|p| super::api::unhooked(lua, p)).collect::<mlua::Result<Vec<_>>>()?;
            let info = info(lua);
            let mut f = info.borrow_mut();
            let Some(s) = f.shaders.iter_mut().find(|s| s.0 == me && s.1 == id) else {
                return Err(mlua::Error::runtime(format!("no shader {id}")));
            };
            // Each a {x, y, z, w} or a plain list; a value left out keeps what it was.
            for (k, v) in params.iter().take(4).enumerate() {
                for (j, name) in ["x", "y", "z", "w"].into_iter().enumerate() {
                    if let Some(x) = v.get::<Option<f32>>(name)?.or(v.get::<Option<f32>>(j as i64 + 1)?) {
                        s.4[k][j] = x;
                    }
                }
            }
            Ok(())
        })?,
    )?;
    // 1 (the game's distance) until a mod sets one, as render.camera() gives it.
    t.set("camera_distance", lua.create_function(|lua, ()| Ok(super::super::camera::scale(&world(lua)?.camst)))?)?;
    t.set(
        "set_camera_distance",
        lua.create_function(|lua, k: f32| {
            super::api::push_op(lua, super::Op::CameraDistance(k))?;
            Ok(())
        })?,
    )?;
    t.set(
        "camera",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let st = &w.camst;
            let out = lua.create_table()?;
            out.set("distance", if st.dist > 0.0 { st.dist } else { 1.0 })?;
            out.set("max_distance", super::super::camera::max_dist(st))?;
            out.set("speed", super::super::camera::speed(st))?;
            out.set("battle_distance", st.battle)?;
            out.set("first_person", st.first)?;
            let c = super::super::camera::view(w);
            for (k, v) in [("eye", c.eye), ("look", c.target)] {
                let p = lua.create_table()?;
                p.set("x", v.x)?;
                p.set("y", v.y)?;
                p.set("z", v.z)?;
                out.set(k, p)?;
            }
            Ok(out)
        })?,
    )?;
    t.set(
        "set_camera",
        lua.create_function(|lua, o: Table| {
            let battle = match o.get::<Value>("battle_distance")? {
                Value::Boolean(false) => Some(None),
                Value::Nil => None,
                v => Some(Some(lua.unpack::<f32>(v)?)),
            };
            // The new limits first, so a distance given with them is clamped to them.
            super::api::push_op(lua, super::Op::Camera(o.get("speed")?, o.get("max_distance")?, battle))?;
            if let Some(d) = o.get::<Option<f32>>("distance")? {
                super::api::push_op(lua, super::Op::CameraDistance(d))?;
            }
            Ok(())
        })?,
    )?;
    g.set("render", t)?;
    Ok(())
}
