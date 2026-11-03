//! Render overrides for mods (docs/lua-mods.md 4.11): how an entity draws (hidden, name, name colour, alpha, tint,
//! size) and the camera's distance. Looks last while the mod is loaded; the newest mod's wins per entity.

use super::api::{state, world};
use mlua::{Lua, Table, Value};

/// One entity's drawn look as a mod set it.
#[derive(Clone, Default)]
pub(in crate::client) struct Look {
    pub hidden: bool,
    pub name: Option<String>,
    /// RGBA, 0..255.
    pub name_colour: Option<[u8; 4]>,
    /// 0..1, at most the model's own.
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

fn info(lua: &Lua) -> std::rc::Rc<std::cell::RefCell<super::state::FrameInfo>> {
    lua.app_data_ref::<std::rc::Rc<std::cell::RefCell<super::state::FrameInfo>>>().map(|f| f.clone()).unwrap_or_default()
}

pub(super) fn colour(v: Value) -> mlua::Result<Option<[u8; 4]>> {
    Ok(match v {
        Value::Table(t) => {
            let get = |k: &str, i: i64, d: u8| -> mlua::Result<u8> { Ok(t.get::<Option<u8>>(k)?.or(t.get::<Option<u8>>(i)?).unwrap_or(d)) };
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
                    let get = |k: &str, i: i64| -> mlua::Result<f32> { Ok(c.get::<Option<f32>>(k)?.or(c.get::<Option<f32>>(i)?).unwrap_or(1.0)) };
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
        "clear",
        lua.create_function(move |lua, ()| {
            info(lua).borrow_mut().looks.retain(|_, l| l.0 != me);
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
            let info = info(lua);
            let mut f = info.borrow_mut();
            let Some(s) = f.shaders.iter_mut().find(|s| s.0 == me && s.1 == id) else { return Err(mlua::Error::runtime(format!("no shader {id}"))) };
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
    t.set("camera_distance", lua.create_function(|lua, ()| Ok(world(lua)?.camst.dist))?)?;
    t.set(
        "set_camera_distance",
        lua.create_function(|lua, k: f32| {
            state(lua).borrow_mut().ops.push(super::Op::CameraDistance(k));
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
                (p.set("x", v.x)?, p.set("y", v.y)?, p.set("z", v.z)?);
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
            state(lua).borrow_mut().ops.push(super::Op::Camera(o.get("speed")?, o.get("max_distance")?, battle));
            if let Some(d) = o.get::<Option<f32>>("distance")? {
                state(lua).borrow_mut().ops.push(super::Op::CameraDistance(d));
            }
            Ok(())
        })?,
    )?;
    g.set("render", t)?;
    Ok(())
}
