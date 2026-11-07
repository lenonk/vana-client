//! The rest of the game façade (docs/lua-mods.md 4.6): skills, jobs, merits and job points, stats, gil, what we have
//! learned, job setups, session flags, the camera, targets by tag and the zone's NPC list.

use super::api::world;
use super::World;
use mlua::{Lua, Table};

/// What the mod phase reads from the UI for the mods (shared as app data): the map grid as <pos> shows it.
#[derive(Default)]
pub(super) struct FrameInfo {
    pub grid: String,
    grid_at: Option<std::time::Instant>,
    /// The window in pixels, and UI units per pixel (the classic layout's stretch).
    pub window: [f32; 2],
    pub k: (f32, f32),
    /// The window's place on the desktop and whether it has the keyboard.
    pub win_pos: Option<(i32, i32)>,
    pub focused: bool,
    /// The HUD elements drawn last frame in window pixels: (mod, element, x y w h).
    pub hud_rects: Vec<(String, String, [f32; 4])>,
    /// Native HUD pieces and modern panels hidden (ui.native.hide): name -> the mods hiding it.
    pub hidden: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    /// Modern panels mods placed, scaled or faded (ui.native.place): panel -> (mod, place).
    pub placed: std::collections::BTreeMap<String, (String, Placed)>,
    /// The modern panels as last drawn, in window pixels, and whether drawn last frame (ui.native.rect).
    pub native_rects: std::collections::BTreeMap<String, ([f32; 4], bool)>,
    /// The modern UI's look mods changed (ui.skin).
    pub skin: super::skin::Skin,
    /// This frame's view-projection, for draw.to_screen.
    pub view_proj: glam::Mat4,
    /// Entity looks mods set (render.entity): server id -> (mod, look).
    pub looks: std::collections::HashMap<u32, (String, super::render::Look)>,
    /// Item name colours mods set for the game's lists (ui.native.item_colour): item -> (mod, game tint).
    pub item_tints: std::collections::HashMap<u16, (String, u32)>,
    /// Post-process passes mods set (render.shader), in order: mod, id, version, WGSL, parameters.
    pub shaders: Vec<crate::render::postfx::Want>,
    pub shader_ver: u64,
}

/// A modern panel's place from a mod: its top left in window pixels (negative from the right or bottom edge; none
/// keeps the panel's own), its scale, the point it scales about (a share of its width and height), its opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub scale: f32,
    pub alpha: f32,
    pub origin: (f32, f32),
}

impl FrameInfo {
    /// The grid square, every half second (both UIs' location panels do the same).
    pub fn update(&mut self, w: &World, gu: &super::super::game_ui::GameUi) {
        if self.grid_at.is_none_or(|t| t.elapsed().as_secs_f32() >= 0.5) {
            self.grid_at = Some(std::time::Instant::now());
            self.grid = super::super::map::grid_text(w, gu.map_regions());
        }
    }
}

fn vec3(lua: &Lua, v: glam::Vec3) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("x", v.x)?;
    t.set("y", v.y)?;
    t.set("z", v.z)?;
    Ok(t)
}

/// A target tag ("t", "<st>", "bt", "pet", "me", "ht", "scan", "p0".."p5", "a10".."a25"), a player's name or a server
/// id: (id, index).
pub(super) fn by_tag(w: &World, tag: &str) -> Option<(u32, u16)> {
    let inner = tag.trim().trim_start_matches('<').trim_end_matches('>').to_ascii_lowercase();
    let slot = match inner.as_bytes() {
        [b'p', d @ b'0'..=b'5'] => Some((d - b'0') as usize),
        [b'a', p @ b'1'..=b'2', d @ b'0'..=b'5'] => Some((p - b'0') as usize * 6 + (d - b'0') as usize),
        _ => None,
    };
    if let Some(k) = slot {
        let m = w.party.slots().slots.get(k).cloned().flatten()?;
        return Some((m.id, m.index));
    }
    let tagged = format!("<{inner}>");
    let probe = if ["t", "me", "bt", "scan", "pet", "ht"].contains(&inner.as_str()) { tagged.as_str() } else { tag.trim() };
    super::super::textcmd::resolve(w, Some(probe), false, true, super::super::textcmd::Mode::Internal)
}

pub(super) fn install(lua: &Lua, game: &Table) -> mlua::Result<()> {
    game.set(
        "entity_by_tag",
        lua.create_function(|lua, tag: String| {
            let w = world(lua)?;
            let Some((id, _)) = by_tag(w, &tag) else { return Ok(None) };
            if id == w.my_id {
                return super::api::self_entity(lua, w).map(Some);
            }
            w.entities.get(&id).map(|e| super::api::entity(lua, w, id, e)).transpose()
        })?,
    )?;
    game.set(
        "skills",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let out = lua.create_table()?;
            for (key, magic) in [("combat", false), ("magic", true)] {
                let t = lua.create_table()?;
                for (id, v) in super::super::status::skill_rows(w, magic) {
                    let s = lua.create_table()?;
                    s.set("level", v & 0x7fff)?;
                    s.set("capped", v & 0x8000 != 0)?;
                    t.set(id, s)?;
                }
                out.set(key, t)?;
            }
            let crafts = lua.create_table()?;
            for (id, v) in super::super::status::craft_rows(w) {
                let s = lua.create_table()?;
                s.set("rank", v & 0x1f)?;
                s.set("level", (v >> 5) & 0x3ff)?;
                s.set("capped", v & 0x8000 != 0)?;
                crafts.set(id, s)?;
            }
            out.set("crafts", crafts)?;
            Ok(out)
        })?,
    )?;
    game.set(
        "jobs",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let out = lua.create_table()?;
            let levels = lua.create_table()?;
            for (job, lv) in super::super::status::job_rows(w, false) {
                levels.set(job, lv)?;
            }
            let master = lua.create_table()?;
            for (job, lv) in super::super::status::job_rows(w, true) {
                master.set(job, lv)?;
            }
            out.set("levels", levels)?;
            out.set("master", master)?;
            Ok(out)
        })?,
    )?;
    game.set(
        "stats",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let Some(s) = w.stats.as_ref() else { return Ok(None) };
            let t = lua.create_table()?;
            t.set("base", s.base.to_vec())?;
            t.set("bonus", s.adj.to_vec())?;
            t.set("attack", s.atk)?;
            t.set("defense", s.def)?;
            t.set("resist", s.elem.to_vec())?;
            t.set("item_level", s.ilvl)?;
            t.set("superior_level", s.su)?;
            t.set("title", s.title)?;
            t.set("rank", s.rank)?;
            t.set("rank_points", s.rank_points)?;
            t.set("nation", s.nation)?;
            t.set("home_point", s.home)?;
            t.set("exp", s.exp.0)?;
            t.set("exp_next", s.exp.1)?;
            t.set("master_level", s.ml.0)?;
            t.set("exemplar", s.ml_exp.0)?;
            t.set("exemplar_next", s.ml_exp.1)?;
            if let Some((limit, bits, max)) = w.merits {
                t.set("limit_points", limit)?;
                t.set("merit_points", bits & 0x7f)?;
                t.set("merit_max", max)?;
            }
            Ok(Some(t))
        })?,
    )?;
    game.set(
        "merits",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let out = lua.create_table()?;
            for (&id, &(next, level)) in &w.merit_list {
                let m = lua.create_table()?;
                m.set("level", level)?;
                m.set("next_cost", next)?;
                out.set(id, m)?;
            }
            Ok(out)
        })?,
    )?;
    game.set(
        "job_points",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let out = lua.create_table()?;
            if let Some(jobs) = &w.jp_jobs {
                for (job, &[capacity, current, spent]) in jobs.iter().enumerate() {
                    let j = lua.create_table()?;
                    j.set("capacity", capacity)?;
                    j.set("points", current)?;
                    j.set("spent", spent)?;
                    out.set(job, j)?;
                }
            }
            Ok(out)
        })?,
    )?;
    game.set("gil", lua.create_function(|lua, ()| Ok(world(lua)?.inv.gil()))?)?;
    game.set(
        "spells",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            Ok((0..(w.spells_known.len() * 8) as u16).filter(|&s| w.spells_known.get(s as usize / 8).is_some_and(|b| b & (1 << (s % 8)) != 0)).collect::<Vec<u16>>())
        })?,
    )?;
    game.set(
        "abilities",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let Some(acts) = w.actions.as_ref() else { return Ok(Vec::new()) };
            Ok(super::super::magic::ability_rows(w, 0).into_iter().filter_map(|i| acts.abilities.get(i as usize).map(|a| a.id)).collect::<Vec<u16>>())
        })?,
    )?;
    game.set(
        "job_data",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let out = lua.create_table()?;
            for (side, key) in [(0usize, "main"), (1, "sub")] {
                let b = &w.ext_job[side];
                if b[0] == 0 {
                    continue;
                }
                let t = lua.create_table()?;
                t.set("job", b[0])?;
                t.set("raw", lua.create_string(&b[..])?)?;
                match b[0] {
                    // BLU: 20 slots, spell id - 0x200.
                    0x10 => t.set("blue_spells", b[4..24].iter().filter(|&&s| s != 0).map(|&s| 0x200 + s as u16).collect::<Vec<u16>>())?,
                    // PUP: head and frame items 0x2000 + n, attachments 0x2100 + n.
                    0x12 => {
                        let item = |i: usize, v: u8| (v != 0).then(|| if i < 2 { 0x2000 + v as u16 } else { 0x2100 + v as u16 });
                        t.set("head", item(0, b[4]))?;
                        t.set("frame", item(1, b[5]))?;
                        t.set("attachments", (2..14).filter_map(|i| item(i, b[4 + i])).collect::<Vec<u16>>())?;
                    }
                    _ => {}
                }
                out.set(key, t)?;
            }
            Ok(out)
        })?,
    )?;
    game.set(
        "macros",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let Some(m) = w.macros.as_ref() else { return Ok(None) };
            let t = lua.create_table()?;
            t.set("book", m.book + 1)?;
            t.set("set", m.set + 1)?;
            t.set("book_title", m.book_title(m.book))?;
            let books = lua.create_table()?;
            for b in 0..40 {
                books.push(m.book_title(b))?;
            }
            t.set("books", books)?;
            for (row, base) in [("ctrl", 0), ("alt", 10)] {
                let r = lua.create_table()?;
                for n in 0..10 {
                    let Some(rec) = m.recs.get(base + n) else { continue };
                    let e = lua.create_table()?;
                    e.set("title", rec.title())?;
                    let lines = lua.create_table()?;
                    for k in 0..6 {
                        lines.push(crate::dat::font::decode_sjis(rec.line(k)))?;
                    }
                    e.set("lines", lines)?;
                    r.push(e)?;
                }
                t.set(row, r)?;
            }
            Ok(Some(t))
        })?,
    )?;
    game.set(
        "map",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let gu = super::api::gu(lua)?;
            let Some(r) = super::super::map::current(w, gu.map_regions()) else { return Ok(None) };
            let t = lua.create_table()?;
            t.set("zone", r.zone)?;
            t.set("number", r.num)?;
            t.set("file", r.file_id())?;
            t.set("scale", r.scale)?;
            let o = lua.create_table()?;
            (o.set("x", r.ox)?, o.set("y", r.oy)?);
            t.set("origin", o)?;
            t.set("size", 512)?;
            t.set("px_per_unit", super::super::minimap::px_per_unit(&r))?;
            Ok(Some(t))
        })?,
    )?;
    game.set(
        "map_pos",
        lua.create_function(|lua, (x, z): (f32, f32)| {
            let w = world(lua)?;
            let gu = super::api::gu(lua)?;
            let Some(r) = super::super::map::current(w, gu.map_regions()) else { return Ok((None, None)) };
            // The pixel on the map's 512 x 512 image (its top left at the map pixel `origin`).
            let (px, py) = r.px(x, z);
            Ok((Some(px - r.ox as i32), Some(py - r.oy as i32)))
        })?,
    )?;
    // The zone's collision in world coordinates (y down, as positions).
    let point = |lua: &Lua, v: glam::Vec3| -> mlua::Result<Table> {
        let p = lua.create_table()?;
        (p.set("x", v.x)?, p.set("y", v.y)?, p.set("z", v.z)?);
        Ok(p)
    };
    game.set(
        "collision",
        lua.create_function(move |lua, (x, z, r): (f32, f32, Option<f32>)| {
            let w = world(lua)?;
            let Some(g) = w.ground.as_ref() else { return Ok(None) };
            let out = lua.create_table()?;
            for (t, n, kind) in g.tris_near(x, z, r.unwrap_or(8.0).clamp(0.0, 64.0)) {
                let e = lua.create_table()?;
                (e.set(1, point(lua, t[0])?)?, e.set(2, point(lua, t[1])?)?, e.set(3, point(lua, t[2])?)?);
                (e.set("normal", point(lua, n)?)?, e.set("kind", kind)?);
                out.push(e)?;
            }
            Ok(Some(out))
        })?,
    )?;
    game.set(
        "collision_mesh",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let Some(g) = w.ground.as_ref() else { return Ok(None) };
            let mut b = Vec::new();
            for t in g.all_tris() {
                for v in t {
                    for c in [v.x, v.y, v.z] {
                        b.extend_from_slice(&c.to_le_bytes());
                    }
                }
            }
            lua.create_string(b).map(Some)
        })?,
    )?;
    game.set(
        "raycast",
        lua.create_function(move |lua, (a, b): (Table, Table)| {
            let w = world(lua)?;
            let v = |t: &Table| -> mlua::Result<glam::Vec3> { Ok(glam::Vec3::new(t.get("x")?, t.get("y")?, t.get("z")?)) };
            let (from, to) = (v(&a)?, v(&b)?);
            let Some(d) = w.ground.as_ref().and_then(|g| g.raycast(from, to)) else { return Ok(None) };
            let hit = point(lua, from + (to - from).normalize_or_zero() * d)?;
            hit.set("distance", d)?;
            Ok(Some(hit))
        })?,
    )?;
    game.set(
        "floor",
        lua.create_function(|lua, (x, z, y): (f32, f32, Option<f32>)| {
            let w = world(lua)?;
            Ok(w.ground.as_ref().and_then(|g| g.settle(x, z, y.unwrap_or(w.pos.y))))
        })?,
    )?;
    game.set(
        "cast",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let Some((total, left)) = w.cast_bar else { return Ok(None) };
            let t = lua.create_table()?;
            if w.cast_spell != 0 {
                t.set("spell", w.cast_spell)?;
            }
            // The bar's ticks are 60 a second.
            t.set("total", total / 60.0)?;
            t.set("left", left / 60.0)?;
            t.set("progress", if total > 0.0 { 1.0 - left / total } else { 1.0 })?;
            t.set("event", w.cast_event)?;
            Ok(Some(t))
        })?,
    )?;
    game.set(
        "info",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let t = lua.create_table()?;
            t.set("zone", w.zone)?;
            t.set("server", w.world_name.as_str())?;
            t.set("mog_house", w.room_flag)?;
            t.set("chat_open", w.typing)?;
            t.set("menu_open", super::super::world_window_open(w))?;
            t.set("event", w.event.is_some())?;
            t.set("in_combat", w.my_motion.status == 1)?;
            t.set("locked", w.locked)?;
            t.set("autorun", w.autorun)?;
            t.set("follow", w.follow)?;
            t.set("pet_index", (w.my_pet != 0).then_some(w.my_pet))?;
            let grid = lua.app_data_ref::<std::rc::Rc<std::cell::RefCell<FrameInfo>>>().map(|f| f.borrow().grid.clone()).unwrap_or_default();
            t.set("map_grid", Some(grid).filter(|g| !g.is_empty() && !w.room_flag))?;
            t.set("modern_ui", w.modern)?;
            Ok(t)
        })?,
    )?;
    game.set(
        "camera",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            // The camera as drawn (the wheel's distance in).
            let c = super::super::camera::view(w);
            let t = lua.create_table()?;
            t.set("eye", vec3(lua, c.eye)?)?;
            t.set("look", vec3(lua, c.target)?)?;
            t.set("distance", (c.target - c.eye).length())?;
            Ok(t)
        })?,
    )?;
    game.set(
        "shop",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let Some(rows) = super::super::shop::stock(w) else { return Ok(None) };
            let out = lua.create_table()?;
            for (k, item, price) in rows {
                let r = lua.create_table()?;
                r.set("index", k + 1)?;
                r.set("item", item)?;
                r.set("price", price)?;
                out.push(r)?;
            }
            Ok(Some(out))
        })?,
    )?;
    game.set(
        "npc_names",
        lua.create_function(|lua, ()| {
            let w = world(lua)?;
            let out = lua.create_table()?;
            for (&id, name) in &w.npc_names {
                out.set(id, name.as_str())?;
            }
            Ok(out)
        })?,
    )?;
    Ok(())
}
