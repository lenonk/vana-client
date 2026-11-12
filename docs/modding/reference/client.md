# `client`

`client` controls the client program itself rather than the game: the clipboard, behaviour switches, the game window,
screenshots and video recording, starting other programs, and the client's process (CPU cores and priority). Calls
that ask the client to do something (`copy`, `paste`, `set_option`, `set_window`, `focus`, `screenshot`, `record`,
`stop_recording`) are queued and carried out after the mod phase, and only once the character is in the world: before
login, in the lobby, they raise `not available in the lobby (before login)`. Permissions differ per function: `paste` needs `clipboard`, `set_window` and
`focus` need `multibox`, `record` and `run` need `exec`, `set_affinity` and `set_priority` need `process`; the rest
need none.

```lua
-- /where copies our position to the clipboard.
commands.register("where", function()
  local p = game.player().pos
  client.copy(("%.2f %.2f %.2f"):format(p.x, p.y, p.z))
  print("Position copied.")
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`client.copy(text)`](#clientcopy) | Puts text on the clipboard. |
| [`client.paste(handler)`](#clientpaste) | Reads the clipboard's text and hands it to a function. |
| [`client.option(name)`](#clientoption) | Reads a behaviour switch. |
| [`client.set_option(name, value)`](#clientset_option) | Sets a behaviour switch while the mod is loaded. |
| [`client.window()`](#clientwindow) | The game window's place, size and focus. |
| [`client.set_window(window)`](#clientset_window) | Moves, sizes or restyles the game window. |
| [`client.focus()`](#clientfocus) | Asks for the keyboard focus for this game window. |
| [`client.screenshot(name?)`](#clientscreenshot) | Saves a screenshot of the next frame. |
| [`client.record(name?, options?)`](#clientrecord) | Starts recording the game window to a video file. |
| [`client.stop_recording()`](#clientstop_recording) | Ends the recording. |
| [`client.recording()`](#clientrecording) | The video file being recorded, if any. |
| [`client.run(program, args?, handler?)`](#clientrun) | Starts another program. |
| [`client.process()`](#clientprocess) | The client's process id, cores, affinity and priority. |
| [`client.set_affinity(cores)`](#clientset_affinity) | Chooses which CPU cores the client runs on. |
| [`client.set_priority(level)`](#clientset_priority) | Sets the client's scheduling priority. |

## `client.copy`

```lua
client.copy(text) -> nothing
```

Puts text on the system clipboard, replacing what was there, so the player can paste it into another program.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `text` | `string` | yes | The text to copy. |

**Returns** nothing.

**Example**

```lua
-- /copytarget copies the target's name.
commands.register("copytarget", function()
  local t = game.target()
  if t then client.copy(t.name) end
end)
```

Queued: the clipboard changes after the mod phase.

## `client.paste`

```lua
client.paste(handler) -> nothing
```

Reads the clipboard's text after this frame and calls `handler` with it in the next mod phase.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `handler` | `function` | yes | Called as `handler(text)`; `text` is a string, `""` when the clipboard holds no text. |

**Returns** nothing.

**Needs** the `clipboard` permission.

**Errors** `this needs the clipboard permission: ...` when the mod does not hold it.

**Example**

```lua
-- /pasteit prints what is on the clipboard.
commands.register("pasteit", function()
  client.paste(function(text)
    print(text == "" and "The clipboard has no text." or ("Clipboard: " .. text))
  end)
end)
```

In the lobby it raises `not available in the lobby (before login)`.

## `client.option`

```lua
client.option(name) -> boolean|number|table
```

Reads one of the behaviour switches (see [`client.set_option`](#clientset_option) for the list) as it is now,
whether your mod, another mod or nobody set it.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | yes | The switch's name, e.g. `"cutscene_speed"`. |

**Returns** `boolean|number|table`: the switch's value, of the type the table under `client.set_option` gives. For
`clear_colour` and `ambient` it is a table `{r, g, b}` (0..255) while one is set, else `false`.

**Errors** `no option "<name>" (<every switch name>)` for an unknown name; `game state is not available here ...` in
the lobby or inside a chat filter.

**Example**

```lua
print("Cutscene speed: " .. client.option("cutscene_speed"))
local bg = client.option("clear_colour")
print(bg and ("Background %d %d %d"):format(bg.r, bg.g, bg.b) or "The zone's background")
```

A value set with `client.set_option` reads back from the next frame on.

## `client.set_option`

```lua
client.set_option(name, value) -> nothing
```

Sets one of the client's behaviour switches. Most lift one of the game's waits or rules, or change how the scene is
drawn. A switch stays set while your mod is loaded: when the mod that set it last unloads, it goes back to its default
(except `minimap_span` and `map_opacity`, see below).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | yes | The switch's name (below). |
| `value` | `boolean`, `number` or `table` | yes | The new value, of the switch's type. |

**The switches**

| Name | Type | Range | Default | Effect |
| --- | --- | --- | --- | --- |
| `macro_palette_delay` | `boolean` | | `true` | `false`: the Ctrl / Alt macro palette opens at once, not at the next whole second. |
| `ah_result_delay` | `boolean` | | `true` | `false`: an auction house bid's result is printed at once. |
| `equipset_delay` | `boolean` | | `true` | `false`: no 0.9 second wait between equip set changes. |
| `log_slide` | `boolean` | | `true` | `false`: new chat log lines appear at once instead of sliding in (in every log window). |
| `cutscene_speed` | `number` | 1..4 | `1` | Events (cutscenes, NPC conversations) play this many times faster. |
| `animation_rate` | `number` | 0.25..4 | `1` | The speed of every character's and monster's animations. |
| `clear_colour` | `{r, g, b}` or `false` | 0..255 each | `false` | The scene's background colour in place of the zone's; `false` (or `nil`) gives the zone's back. |
| `ambient` | `{r, g, b}` or `false` | 0..255 each | `false` | The ambient light in place of the zone's; `false` (or `nil`) gives the zone's back. |
| `minimap_span` | `number` | 48..512 | the player's zoom (160 at first) | The minimap's zoom: how many map pixels it shows across (the Minimap mod sets it from the wheel over its map). It is kept, and not put back when the mod unloads. |
| `map_opacity` | `number` | 0..1 | `0.6` | How opaque the HUD maps are where a mod gives no `alpha`, and how strongly the minimap's marker is shaded (as the map overlay's). The Map Overlay mod sets it from its Map Overlay Opacity setting. It is the player's choice: it is kept, not put back when the mod unloads, and the client remembers it across sessions, so it holds while the Map Overlay mod is off. |
| `move_lock` | `boolean` | | `true` | `false`: we can move while using an ability or drawing our weapon would normally hold us in place. |
| `all_maps` | `boolean` | | `false` | `true`: every map can be opened without its key item. |
| `dead_commands` | `boolean` | | `false` | `true`: text commands work while K.O. |
| `compass_monsters` | `boolean` | | `false` | `true`: monsters show as dots on the compass for every job. |
| `player_collision` | `boolean` | | `true` | `false`: other players never hold up our step (normally a moving player can; NPCs still do). |
| `occlusion` | `boolean` | | `true` | `false`: characters behind scenery are still drawn and can still be targeted. |
| `menu_reach` | `boolean` | | `true` | `false`: the auction house and nomad moogle menus stay open when we walk away (normally they close beyond 6 yalms). |
| `send_interval` | `number` | 50..1000 | `400` | Milliseconds between the packets the client sends to the game server. |
| `wireframe` | `boolean` | | `false` | `true`: the scenery is drawn as lines, where the graphics card can draw them. |
| `ttf_text` | `boolean` | | `false` | `true`: in the classic interface too, the game font's text (the log, the windows' text) is drawn in the interface's TrueType face; sprite numbers and captions stay as they are. |
| `stamp_all_lines` | `boolean` | | `false` | `true`: the game's chat timestamps (its Config's) go on every log line, not only chat lines; with the game's timestamps off, every line gets hours, minutes and seconds. The bundled Timestamps mod sets it. |
| `synth_amount` | `boolean` | | `false` | `true`: after Synthesize the synthesis window asks how many (1 to the sets held) before the first goes out, and gives the count to the [`synth_amount`](events.md#synth_amount) event. The bundled Synthesis Amount mod sets it. |

Numbers outside a range are clamped to it. A colour is `{r = ..., g = ..., b = ...}` or `{r, g, b}`.

**Returns** nothing.

**Errors** `no option "<name>" (<every switch name>)` for an unknown name; `option <name>: true or false`, `option
<name>: a number` or `option <name>: a colour {r, g, b} (0..255), or false` for a value of the wrong type; a conversion
error for a value that is not plain data (a function, for example).

**Example**

```lua
-- Faster cutscenes, no wait between equip sets, and a dark red background.
client.set_option("cutscene_speed", 2)
client.set_option("equipset_delay", false)
client.set_option("clear_colour", { r = 60, g = 0, b = 0 })
```

Queued: applied after the mod phase. A value of the wrong type (a number for a `boolean` switch, a colour table
missing `g`) is ignored without an error, so read it back with `client.option` the next frame if in doubt. When
several mods set the same switch, the last one wins, and the switch goes back to its default when that mod unloads.
Camera settings are in [`render.set_camera`](render.md#renderset_camera).

## `client.window`

```lua
client.window() -> {x: integer?, y: integer?, w: integer, h: integer, focused: boolean}
```

The game window's place on the desktop, its size and whether it has the keyboard focus, as of the last frame.

**Returns** `table`:

| Field | Type | Description |
| --- | --- | --- |
| `x` | `integer?` | The window's left edge on the desktop, in pixels; `nil` when the system does not tell. |
| `y` | `integer?` | Its top edge; `nil` when the system does not tell. |
| `w` | `integer` | The window's width in pixels. |
| `h` | `integer` | Its height in pixels. |
| `focused` | `boolean` | Whether the window has the keyboard focus. |

**Example**

```lua
local win = client.window()
print(("Window %dx%d, focused: %s"):format(win.w, win.h, tostring(win.focused)))
if win.x then print(("At %d, %d on the desktop"):format(win.x, win.y)) end
```

The `focus_change` event tells you when the focus changes.

## `client.set_window`

```lua
client.set_window(window) -> nothing
```

Changes the game window: moves it, sizes it, keeps it on top, minimises it, makes it full screen, shows or hides its
frame, or keeps it out of screen captures. Only the fields you give change. Use it to arrange several game windows.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `window` | `table` | yes | What to change (below). |

**`window` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `x`, `y` | `integer` | unchanged | Moves the window's top-left corner to this desktop position, in pixels. Give both, or neither is used. |
| `w`, `h` | `integer` | unchanged | Sizes the window, in pixels (at least 320 x 240). Give both, or neither is used. |
| `top` | `boolean` | unchanged | `true` keeps the window above other windows; `false` makes it a normal window again. |
| `minimised` | `boolean` | unchanged | `true` minimises the window; `false` restores it. |
| `fullscreen` | `boolean` | unchanged | `true` makes it borderless full screen on its monitor; `false` makes it a window again. |
| `border` | `boolean` | unchanged | `true` shows the system's frame and title bar; `false` hides them. |
| `capture` | `boolean` | unchanged | `false` keeps the window out of screen captures by other programs, where the system allows it; `true` allows them again. |

**Returns** nothing.

**Needs** the `multibox` permission.

**Errors** `this needs the multibox permission: ...` when the mod does not hold it; a conversion error for a field of
the wrong type (for example a negative `w`).

**Example**

```lua
-- /corner: a 960 x 540 window without a frame in the desktop's top-left corner, kept on top.
commands.register("corner", function()
  client.set_window({ x = 0, y = 0, w = 960, h = 540, top = true, border = false })
end)
```

Queued: the window changes after the mod phase. The desktop has the last word: some systems ignore moves or
always-on-top requests.

## `client.focus`

```lua
client.focus() -> nothing
```

Asks the desktop to give this game window the keyboard focus, for switching between several clients.

**Returns** nothing.

**Needs** the `multibox` permission.

**Errors** `this needs the multibox permission: ...` when the mod does not hold it.

**Example**

```lua
-- Take the focus when another of our clients sends "focus <our name>" with ipc.send.
events.on("ipc_message", function(text)
  if text == "focus " .. game.player().name then
    client.focus()
  end
end)
```

Queued: the request is made after the mod phase; the desktop may refuse it.

## `client.screenshot`

```lua
client.screenshot(name?) -> string
```

Saves a PNG screenshot of the next frame, the whole window as drawn with the interface, in the game's screenshots
folder. The name may include subfolders, which are made as needed.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | no, default `"mod"` + the date and time | The file name relative to the screenshots folder, e.g. `"boss/phase2"`. `.png` is added unless the name already ends in `.png`. |

**Returns** `string`: the name used, relative to the screenshots folder (for example `"boss/phase2.png"`, or
`"mod261102143005.png"` for the default: year, month, day, hour, minute, second).

**Errors** `"<name>": a name under the screenshots folder` when the name is an absolute path, starts with `./`, or
contains a `..` part.

**Example**

```lua
-- /snap saves a screenshot in a "mods" subfolder.
commands.register("snap", function()
  local name = client.screenshot("mods/snap")
  print("Saved screenshots/" .. name)
end)
```

Queued: the picture is taken from the next frame, so the file appears a moment later. In the lobby it raises
`not available in the lobby (before login)`.

## `client.record`

```lua
client.record(name?, options?) -> string
```

Starts recording the game window, as drawn with the interface, to an H.264 video (`.mp4`, no sound). The video is
encoded by `ffmpeg`, which must be installed. The recording runs until `client.stop_recording`, until the window
changes size, or until your mod unloads.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | no, default the date and time (`20261102-143005`) | The file name without `.mp4`; `/` and `\` in it become `_`. When that file exists, `-2`, `-3`... is added: an earlier video is never replaced. |
| `options` | `table` | no | Recording options (below). |

**`options` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `fps` | `integer` | `30` | Frames a second in the video, 1..120 (clamped). |

**Returns** `string`: the full path of the video file, in the `ffxiuser/videos` folder of the client's data folder.

**Needs** the `exec` permission.

**Errors** `this needs the exec permission: ...` when the mod does not hold it; `<mod> is recording` while another mod's
recording runs; a conversion error when `options` is not a table.

**Example**

```lua
-- /clip records ten seconds at 60 frames a second.
commands.register("clip", function()
  local path = client.record("clip", { fps = 60 })
  sleep(10)
  client.stop_recording()
  print("Saved " .. path)
end)
```

Queued: the recording starts after the mod phase. Only one recording runs at a time: your mod starting a new one ends
its running one, and another mod's raises the error above. If the recording cannot start (for example `ffmpeg` is
not installed), nothing is recorded, `client.recording()` stays `nil`, and a red line in the chat log gives the reason.

## `client.stop_recording`

```lua
client.stop_recording() -> nothing
```

Ends your mod's recording; `ffmpeg` then finishes the file. Another mod's recording goes on.

**Returns** nothing.

**Example**

```lua
-- Stop recording when we change zones.
events.on("zone_change", function()
  if client.recording() then
    client.stop_recording()
  end
end)
```

Queued: the recording ends after the mod phase.

## `client.recording`

```lua
client.recording() -> string?
```

The video file being recorded, as of the last frame.

**Returns** `string?`: the file's full path, or `nil` when nothing is being recorded. Right after `client.record` it is
still `nil` until the recording has started (the next frame).

**Example**

```lua
-- A red REC mark while a recording runs.
timer.every(1, function()
  hud.text("rec", { x = 10, y = 10, text = client.recording() and "REC" or "", colour = { 255, 0, 0 } })
end)
```

## `client.run`

```lua
client.run(program, args?, handler?) -> integer
```

Starts another program with its arguments, without a shell (so no `|`, `>` or `*` expansion: each argument is passed
as it is). It gets no input. With a `handler`, the program's output is collected and handed to the handler when the
program ends; without one, its output is thrown away.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `program` | `string` | yes | The program: a path, or a name looked up the way the system finds programs. |
| `args` | `string[]` | no, default none | Its arguments, one string each. |
| `handler` | `function` | no | Called as `handler(result)` in a later mod phase when the program ends (fields below). |

**`result` fields** (what `handler` gets)

| Field | Type | Description |
| --- | --- | --- |
| `status` | `integer` | The program's exit code (0 usually means success; on Windows the whole 32-bit code, such as 3221225477 for a crash). A program ended by a signal (Linux, macOS) gives minus the signal's number: -9 for a kill, -15 for a terminate. |
| `body` | `string` | Everything the program wrote to its standard output. |
| `headers` | `table` | `{stderr = <text>}`: everything it wrote to its standard error. |
| `error` | `string` | Present instead of the fields above when waiting for the program failed. |

**Returns** `integer`: the started program's process id.

**Needs** the `exec` permission.

**Errors** `this needs the exec permission: ...` when the mod does not hold it; `<program>: <reason>` when the program
cannot be started (for example, it is not found).

**Example**

```lua
-- /up prints how long the computer has been running.
commands.register("up", function()
  client.run("uptime", { "-p" }, function(r)
    if r.error then print("uptime failed: " .. r.error) return end
    print(("exit %d: %s"):format(r.status, r.body))
  end)
end)
```

A program ended by a signal, rather than exiting, also reports `status` 0. The program keeps running if the mod
unloads; its result is then not delivered.

## `client.process`

```lua
client.process() -> {pid: integer, cores: integer, affinity: integer[], priority: string}
```

Facts about the client's process: its id, how many CPU cores the computer has, which cores it may run on, and its
scheduling priority.

**Returns** `table`:

| Field | Type | Description |
| --- | --- | --- |
| `pid` | `integer` | The client's process id. |
| `cores` | `integer` | How many CPU cores the computer has, whatever the affinity. |
| `affinity` | `integer[]` | The cores it may run on, numbered from 0; empty where the system does not tell. |
| `priority` | `string` | The level nearest its current priority: `"idle"`, `"low"`, `"below_normal"`, `"normal"`, `"above_normal"` or `"high"`. |

**Example**

```lua
local p = client.process()
print(("pid %d, %d cores, priority %s"):format(p.pid, p.cores, p.priority))
print("Runs on cores: " .. table.concat(p.affinity, ", "))
```

## `client.set_affinity`

```lua
client.set_affinity(cores) -> boolean, string?
```

Makes every thread of the client run only on the given CPU cores (threads started later too). Use it to keep several
clients, or the client and other heavy programs, off each other's cores.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `cores` | `integer[]` | yes | The core numbers, from 0 up to the number of cores minus one; at least one. |

**Returns** `boolean, string?`: `true` when it worked; `false` and the reason when it did not: `no core <n> (cores
count from 0)` for a number past what the system can address, else the system's reason (for example, none of the
cores exists).

**Needs** the `process` permission.

**Errors** `set_affinity needs at least one core (from 0)` for an empty list; `this needs the process permission: ...`
when the mod does not hold it.

**Example**

```lua
-- Keep the client off cores 0 and 1.
local keep = {}
for _, c in ipairs(client.process().affinity) do
  if c > 1 then keep[#keep + 1] = c end
end
if #keep > 0 then
  local ok, why = client.set_affinity(keep)
  if not ok then print("set_affinity: " .. why) end
end
```

It takes effect at once and is not undone when the mod unloads.

## `client.set_priority`

```lua
client.set_priority(level) -> boolean, string?
```

Sets the client's scheduling priority, for every thread: how the system shares the CPU between it and other programs.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `level` | `string` | yes | One of `"idle"`, `"low"`, `"below_normal"`, `"normal"`, `"above_normal"`, `"high"` (lowest first). |

**Returns** `boolean, string?`: `true` when it worked; `false` and the system's reason when it did not.

**Needs** the `process` permission.

**Errors** `priority: idle, low, below_normal, normal, above_normal, high` for any other level;
`this needs the process permission: ...` when the mod does not hold it.

**Example**

```lua
-- A second client that should not slow the first one down.
local ok, why = client.set_priority("below_normal")
print(ok and "Priority lowered." or ("set_priority: " .. why))
```

It takes effect at once and is not undone when the mod unloads. On Linux, raising the priority above where it is now
(to `above_normal` or `high`, or back up after lowering it) usually needs extra privileges, so it returns `false` and
the reason; plan for a lowered priority staying lowered. On Windows `low` and `below_normal` are the same class.
