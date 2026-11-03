# Globals

Every mod has these global functions: `print` shows a line in the chat log, `require` loads more of the mod's Lua
files, `sleep`, `sleep_frames` and `wait_until` pause the running handler while the game goes on, and `clock` measures
time. The `mod` table describes the running mod. They need no permission and work anywhere, but the three waiting
functions only make sense in a handler: every handler (events, commands, timers, windows, the entry file itself) runs
as a coroutine that may pause, while filters and packet hooks must return at once and cannot wait.

```lua
local util = require("lib.util")   -- lib/util.lua in the mod's folder

commands.register("countdown", function()
  for n = 3, 1, -1 do
    print(n)
    sleep(1)
  end
  print("Go! (" .. util.name() .. ")")
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`print(...)`](#print) | Shows its arguments as a line in the chat log (and in vana-client.log). |
| [`require(name)`](#require) | Loads a Lua file of the mod once and returns what it returned. |
| [`sleep(seconds?)`](#sleep) | Pauses the running handler for some seconds. |
| [`sleep_frames(n?)`](#sleep_frames) | Pauses the running handler for some frames. |
| [`wait_until(test, timeout?)`](#wait_until) | Pauses the running handler until a test passes or time runs out. |
| [`clock()`](#clock) | Seconds since the mod was loaded. |
| [`mod`](#mod) | A table describing the running mod: id, name, version, API version, folder. |

## `print`

```lua
print(...)
```

Shows a line in the chat log, in a dim colour, and writes it to vana-client.log (prefixed `lua <mod id>: `). Use it
for messages the player should see; for details only you need, use [`log`](log.md).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `...` | `any` | no | Any values. Each is turned into text as `tostring` does (a table's `__tostring` is used), and they are joined with tabs. |

**Returns** nothing.

**Example**

```lua
local p = game.player()
print("HP", p.hp, "MP", p.mp)
print(string.format("%s is at %.1f, %.1f", p.name, p.pos.x, p.pos.z))
```

The line shows once the mod phase is over (at the end of the frame). In the lobby, before login, the line goes to
vana-client.log only.

## `require`

```lua
require(name) -> any
```

Loads another Lua file from the mod's own folder and returns what it returned. A dot in `name` stands for a folder:
`require("lib.util")` loads `lib/util.lua`, or `lib/util/init.lua` when that is what exists. Names are always taken from
the mod's top folder, whichever file calls `require`. Each name is loaded once; later calls return the same value.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | yes | The module's name, with dots between folders and no `.lua`. |

**Returns** `any`: what the file returned, or `true` when it returned nothing. The file gets `name` as its `...`.

**Errors** "require "<name>": outside the mod's folder" for a name with `..` or starting with `/`; "module "<name>" not
found in the mod's folder" when neither file exists; the file's syntax errors, and any error its code raises. A module
that failed is not remembered, so the next `require` tries again.

**Example**

```lua
-- lib/util.lua
local M = {}
function M.name() return mod.name end
return M
```

```lua
-- init.lua
local util = require("lib.util")
print(util.name())
```

`require` reads only the files that came with the mod, never its data folder (see [`fs`](fs.md)). The code a module
runs at its top level cannot `sleep`; put waiting in the functions it returns.

## `sleep`

```lua
sleep(seconds?)
```

Pauses the running handler for at least `seconds`; the game and the mod's other handlers go on meanwhile. The handler
carries on where it stopped, at the first wake-up check after the time is up (the checks run once a frame).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `seconds` | `number` | no, default `0` | How long to wait. A negative number counts as `0`. |

**Returns** nothing.

**Example**

```lua
commands.register("remind", function(minutes, ...)
  local what = table.concat({ ... }, " ")
  sleep((tonumber(minutes) or 1) * 60)
  print("Reminder: " .. what)
end)
```

A handler started by the player's input (a typed command, a click in a mod window) keeps that standing after it
wakes, so it may still do what the `actions` permission allows. If the mod unloads, its paused handlers are dropped.
For the order of wake-ups within a frame, see [`sleep_frames`](#sleep_frames).

## `sleep_frames`

```lua
sleep_frames(n?)
```

Pauses the running handler for `n` frames.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `n` | `integer` | no, default `1` | How many frames to wait, at least 1. Pass an integer: a float such as `2.0` counts as 1. |

**Returns** nothing.

**Example**

```lua
events.on("zone_change", function(zone)
  sleep_frames(1)
  print("Welcome to " .. (res.zone(zone) or "?"))
end)
```

Each frame the client first runs the mod's commands, events and timers, then its wake-up check, then its `frame`
handlers; the actions the mod queued are applied after all of that. A handler that pauses always resumes in a later
frame, never the one it paused in: `sleep_frames(1)` (or `sleep(0)`) resumes in the next frame, after the actions
queued before the pause have been applied.

## `wait_until`

```lua
wait_until(test, timeout?)
```

Pauses the running handler until `test()` returns a true value, or until `timeout` seconds have passed. The client
calls `test` with no arguments at each wake-up check, once a frame (see [`sleep_frames`](#sleep_frames) for when in
the frame).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `test` | `function` | yes | The condition: return anything but `false` or `nil` to resume. |
| `timeout` | `number` | no, default none | Seconds to wait at most. Without it, the handler waits for as long as it takes. |

**Returns** nothing: test the condition again afterwards to tell whether it passed or the time ran out.

**Errors** When `test` is not a function, the mod gets the error "wait_until needs a function" and the handler ends.

**Example**

```lua
commands.register("waitmp", function()
  local function full()
    return (game.player().mpp or 0) >= 100
  end
  wait_until(full, 120)
  print(full() and "MP is full." or "Gave up waiting.")
end)
```

`test` runs every frame, so keep it short. It cannot sleep, and if it raises an error the wait simply ends, without
the error being shown.

## `clock`

```lua
clock() -> number
```

Seconds since the mod was loaded, from a steady clock that changes to the computer's date and time do not affect.
Use it to measure how long something took.

**Returns** `number`: the seconds, with a fraction (sub-millisecond precision).

**Example**

```lua
local t0 = clock()
local n = #game.entities()
print(string.format("%d entities listed in %.2f ms", n, (clock() - t0) * 1000))
```

For the date and time of day, use `os.time` and `os.date`; for the game's clock, `game.time`.

## `mod`

A table describing the running mod, filled in from its `mod.toml` when it loads. Changing its fields changes nothing
else.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | The mod's id: `id` in mod.toml, or the mod folder's name. |
| `name` | `string` | Its display name: `name` in mod.toml, or the id. |
| `version` | `string` | `version` in mod.toml, `"0.0.0"` when it gives none. |
| `api` | `integer` | The mod API version this client provides (currently `1`), not the `api` value in mod.toml. |
| `path` | `string` | The mod's folder on disk; `"bundled/<id>"` for a mod built into the client. |

**Example**

```lua
print(mod.name .. " " .. mod.version .. " (" .. mod.id .. ")")
if mod.api < 2 then
  log.info("running on mod API " .. mod.api)
end
```

The `fs` functions take paths relative to the mod's data folder; they do not need `mod.path`.

## The Lua environment

- Lua 5.4 with the `coroutine`, `math`, `string`, `table` and `utf8` libraries, and `os` with only `os.time`,
  `os.date`, `os.clock` and `os.difftime`. There is no `io`, `package` or `debug`; `dofile` and `loadfile` are absent;
  `load` accepts source text only; `string.dump` is absent; `collectgarbage("count")` gives the mod's memory in KB and
  the other options do nothing.
- Each mod has its own Lua state with up to 64 MB of memory.
- One call into the mod that runs past 100 ms is stopped with the error "ran past its time budget"; a mod that keeps
  raising errors (20 within a minute) is suspended until it is reloaded.
