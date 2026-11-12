//! Sound for mods (docs/lua-mods.md 4.14): the mod's WAV (or SeWave) files and the game's effects through our
//! mixer, at the effects volume.

use mlua::{Lua, Table};
use std::sync::Arc;

/// A mod's hold on the music: a game track by id, or silence.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(in crate::client) enum Music {
    Track(u32),
    Mute,
}

/// A sound a mod asked for, played by the app after the mod phase.
pub(in crate::client) enum Sound {
    /// File bytes and a level 0..127.
    File(Arc<[u8]>, i32),
    /// A game effect by id and level.
    Se(u32, i32),
    /// An interface sound by id.
    Ui(u32),
}

fn level(volume: Option<f32>) -> i32 {
    (volume.unwrap_or(1.0).clamp(0.0, 1.0) * 127.0).round() as i32
}

pub(super) fn install(lua: &Lua, g: &Table, vm: &super::vm::Vm) -> mlua::Result<()> {
    let t = lua.create_table()?;
    let src = vm.src.clone();
    t.set(
        "play",
        lua.create_function(move |lua, (path, volume): (String, Option<f32>)| {
            if super::vm::outside(&path) {
                return Err(mlua::Error::runtime(format!("{path:?}: outside the mod's folder")));
            }
            let bytes: Vec<u8> = match &src {
                super::vm::Source::Dir(d) => super::vm::inside(d, &path)
                    .and_then(std::fs::read)
                    .map_err(|e| mlua::Error::runtime(format!("{path}: {e}")))?,
                super::vm::Source::Bundled(_) => return Err(mlua::Error::runtime("bundled mods carry no sound files")),
            };
            crate::audio::format::Header::parse(&bytes)
                .map_err(|e| mlua::Error::runtime(format!("{path}: {e} (16-bit PCM WAV, mono or stereo)")))?;
            super::api::push_op(lua, super::Op::Sound(Sound::File(bytes.into(), level(volume))))?;
            Ok(())
        })?,
    )?;
    t.set(
        "play_se",
        lua.create_function(|lua, (id, volume): (u32, Option<f32>)| {
            super::api::push_op(lua, super::Op::Sound(Sound::Se(id, level(volume))))?;
            Ok(())
        })?,
    )?;
    t.set(
        "play_ui",
        lua.create_function(|lua, id: u32| {
            super::api::push_op(lua, super::Op::Sound(Sound::Ui(id)))?;
            Ok(())
        })?,
    )?;
    let me = vm.manifest.id.clone();
    t.set(
        "music",
        lua.create_function(move |lua, id: Option<u32>| {
            let m = id.map(Music::Track);
            super::api::push_op(lua, super::Op::Music(me.clone(), m))?;
            Ok(())
        })?,
    )?;
    let me = vm.manifest.id.clone();
    t.set(
        "mute_music",
        lua.create_function(move |lua, on: Option<bool>| {
            let m = on.unwrap_or(true).then_some(Music::Mute);
            super::api::push_op(lua, super::Op::Music(me.clone(), m))?;
            Ok(())
        })?,
    )?;
    // The zone's music slots as the server set them: 1 day, 2 night, 3 solo battle, 4 party battle, 5 mount, 6 death,
    // 7 and 8 from events (0 unset).
    t.set("music_slots", lua.create_function(|lua, ()| Ok(super::api::world(lua)?.music.slots.to_vec()))?)?;
    g.set("audio", t)?;
    Ok(())
}
