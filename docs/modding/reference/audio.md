# `audio`

`audio` plays sounds: WAV files that ship with your mod, the game's sound effects and interface sounds, and it
can take over the background music (hold a track of your choice, or silence it). No permission is needed. Every call
is queued and the sound starts after the mod phase; nothing plays without an audio device, and a sound is skipped
when the game is already playing as many sounds at once as it allows. Sounds play at the player's effects volume, so
a `volume` of 1 is as loud as the game's effects. A mod's hold on the music ends when it gives it back or unloads.

```lua
-- A chime from the mod's folder when we gain a status effect.
events.on("buff_gain", function(id)
  audio.play("sounds/chime.wav", 0.8)
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`audio.play(path, volume?)`](#audioplay) | Plays a sound file from the mod's folder. |
| [`audio.play_se(id, volume?)`](#audioplay_se) | Plays one of the game's sound effects by id. |
| [`audio.play_ui(id)`](#audioplay_ui) | Plays one of the game's interface sounds by id. |
| [`audio.music(id?)`](#audiomusic) | Holds the music on a game track, or gives the music back. |
| [`audio.mute_music(on?)`](#audiomute_music) | Silences the music, or gives it back. |

## `audio.play`

```lua
audio.play(path, volume?) -> nothing
```

Plays a sound file from your mod's folder once. The file is a 16-bit PCM WAV (mono or stereo, any sample rate) or a
game sound file in the SeWave format.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `path` | `string` | yes | The file, relative to the mod's folder (e.g. `"sounds/ding.wav"`). |
| `volume` | `number` | no, default `1` | 0..1 (clamped), scaled by the player's effects volume. |

**Returns** nothing.

**Errors**
- `<path>: outside the mod's folder` when `path` contains `..` or `:`, or starts with `/`.
- `<path>: <reason>` when the file cannot be read (for example, it does not exist).
- `<path>: <reason> (16-bit PCM WAV, mono or stereo)` when the file is not a sound the client can play (for example an
  8-bit or 24-bit WAV).
- `bundled mods carry no sound files` for a mod that ships inside the client.

**Example**

```lua
-- /ding plays the mod's ding.wav at half volume.
commands.register("ding", function()
  audio.play("ding.wav", 0.5)
end)
```

The file is read and checked when you call `audio.play`, so a missing or wrong file raises its error right there;
the sound itself starts after the mod phase.

## `audio.play_se`

```lua
audio.play_se(id, volume?) -> nothing
```

Plays one of the game's sound effects by its id, without a position in the world.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The game's sound effect id. |
| `volume` | `number` | no, default `1` | 0..1 (clamped), scaled by the player's effects volume. |

**Returns** nothing.

**Example**

```lua
-- Play effect 100 at full volume, then again quieter.
commands.register("se", function()
  audio.play_se(100)
  sleep(1)
  audio.play_se(100, 0.3)
end)
```

## `audio.play_ui`

```lua
audio.play_ui(id) -> nothing
```

Plays one of the game's interface sounds by id: the sounds its menus make for a cursor move, a selection, a
cancel and so on. Use it so a mod's own windows and prompts sound like the game's.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The interface sound's id. |

**Returns** nothing.

**Example**

```lua
-- An interface sound when the mod's window opens.
commands.register("panel", function()
  ui.open("panel")
  audio.play_ui(13)
end)
```

Interface sounds play at the game's interface level; there is no volume parameter.

## `audio.music`

```lua
audio.music(id?) -> nothing
```

Holds the background music on one of the game's tracks by id: while your mod holds it, the game stops choosing the
music itself (field, battle, night and the like). Called with no id, it gives the music back and the track the
situation calls for plays again.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | no, default `nil` | The game's music track id; `nil` gives the music back. |

**Returns** nothing.

**Example**

```lua
-- /theme holds track 101 for a minute, then gives the music back.
commands.register("theme", function()
  audio.music(101)
  sleep(60)
  audio.music()
end)
```

Only one mod holds the music at a time: a newer `audio.music(id)` or `audio.mute_music()` from any mod takes it over.
Giving it back (`audio.music()` or `audio.mute_music(false)`) only counts from the mod that holds it. The hold also
ends when the mod unloads.

## `audio.mute_music`

```lua
audio.mute_music(on?) -> nothing
```

Silences the background music (it fades out) and keeps it silent while your mod holds it, or gives the music back.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `on` | `boolean` | no, default `true` | `true` silences the music; `false` gives it back. |

**Returns** nothing.

**Example**

```lua
-- No music while an event (a cutscene) is under way.
local muted = false
events.on("frame", function()
  local busy = game.event() ~= nil
  if busy ~= muted then
    muted = busy
    audio.mute_music(busy)
  end
end)
```

`audio.mute_music(false)` and `audio.music()` do the same thing: they release your mod's hold on the music, whether
it held a track or silence.
