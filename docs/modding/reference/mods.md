# `mods`

The other mods: which are loaded and installed, functions one mod offers the others (`mods.provide` and
`mods.call`), and events one mod sends the others (`mods.emit`). Every value that crosses from one mod to another is
copied as plain data: nil, booleans, finite numbers, text strings and tables of those. A table whose keys are exactly
1..n arrives as a list; any other table arrives with its keys turned into strings (`[5]` becomes `["5"]`). Functions
cannot cross. These calls work in the lobby too, and need no permission.

```lua
-- Mod "tally" counts our hits and offers the count to other mods.
local hits = 0
events.on("damage", function(actor)
  if actor == game.player().id then hits = hits + 1 end
end)
mods.provide("hits", function() return hits end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`mods.list()`](#modslist) | Every loaded mod, with its timings, memory and errors. |
| [`mods.installed()`](#modsinstalled) | Every mod installed, loaded or not. |
| [`mods.provide(name, func)`](#modsprovide) | Offers a function to other mods under a name. |
| [`mods.call(mod, name, ...)`](#modscall) | Calls a function another mod offers, now, and returns its results. |
| [`mods.emit(event, ...)`](#modsemit) | Raises an event in every other mod. |

## `mods.list`

```lua
mods.list() -> table[]
```

Every loaded mod, this one included (a suspended mod too).

**Returns** `table[]`: one table per loaded mod:

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | The mod's id. |
| `name` | `string` | Its name. |
| `version` | `string` | Its version. |
| `provides` | `string[]` | The names it offers with `mods.provide`. |
| `avg_ms` | `number` | A running average of its time per frame, in milliseconds. |
| `peak_ms` | `number` | The most time it took in one frame since it loaded, in milliseconds. |
| `memory_kb` | `integer` | The memory its Lua uses, in KB. |
| `errors` | `integer` | How many errors it had recently: 20 within 60 seconds suspend a mod. |

**Example**

```lua
for _, m in ipairs(mods.list()) do
  print(("%s %s: %.2f ms, %d KB"):format(m.id, m.version, m.avg_ms, m.memory_kb))
end
```

## `mods.installed`

```lua
mods.installed() -> table[]
```

Every mod installed, loaded or not: each folder in the mods folder whose mod.toml reads, and the mods built into the
client (unless a folder of the same id replaces one), sorted by id. Mods whose mod.toml has an error are left out.

**Returns** `table[]`: one table per mod:

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | The mod's id. |
| `name` | `string` | Its name. |
| `version` | `string` | Its version. |
| `enabled` | `boolean` | Whether it is on in the player's mod list (a built-in mod is on unless turned off). |
| `bundled` | `boolean` | Whether it is built into the client. |
| `loaded` | `boolean` | Whether it is running now. |
| `commands` | `string[]` | The commands its mod.toml declares (`[commands.<name>]`), by name. |

**Example**

```lua
for _, m in ipairs(mods.installed()) do
  if m.enabled and not m.loaded then
    print(m.name .. " is enabled but not loaded")
  end
end
```

The player loads and unloads mods with `/lua load <id>` and `/lua unload <id>`. A mod can run the same lines with
`commands.run`, which needs the `actions` or `automation` permission for them.

## `mods.provide`

```lua
mods.provide(name, func)
```

Offers `func` to other mods, which call it with `mods.call(<this mod's id>, name, ...)`. Offering a name again
replaces the function.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | yes | The name callers use. |
| `func` | `function` | yes | Gets the caller's arguments (copies); what it returns goes back to the caller (copies). |

**Returns** nothing.

**Example**

```lua
local notes = {}
mods.provide("add_note", function(text)
  notes[#notes + 1] = text
  return #notes
end)
```

The function runs at once, in this mod, while the caller waits: it cannot sleep, it has about 20 ms, and it reads
game state only when the caller can.

## `mods.call`

```lua
mods.call(mod, name, ...) -> any
```

Calls the function another mod offers under `name` (see `mods.provide`), right now, and returns what it returned.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `mod` | `string` | yes | The other mod's id (case does not matter). |
| `name` | `string` | yes | The name it offers the function under. |
| `...` | plain data | no | Arguments, copied to the other mod. |

**Returns** `any`: every value the function returned, copied back (none when it returned nothing).

**Errors**

- `mods.call: no mod <id> loaded` when no mod of that id is loaded.
- `mods.call: <id> provides no <name>` when it offers nothing under that name.
- `mods.call: <id> is suspended` when that mod is suspended for its errors.
- `mods.call <id>.<name>: <error>` when the function raised an error, tried to sleep, or ran past its time budget.
- `mods.call argument <n> is not plain data` (or `mods.call result <n> ...`, with the path inside a table, e.g.
  `mods.call argument 1.callback`) when a value holds a function or another value that cannot be copied; also an
  error for a number that is not finite, a string that is not UTF-8 text, or tables nested more than 64 deep.

**Example**

```lua
local ok, n = pcall(mods.call, "tally", "hits")
if ok then
  print("Hits so far: " .. n)
else
  print("tally is not available: " .. n)
end
```

## `mods.emit`

```lua
mods.emit(event, ...)
```

Raises `event` in every other running mod: their `events.on(event, ...)` handlers get the values. The sending mod
does not get its event.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `event` | `string` | yes | The event's name. Start it with your mod's id (`"tally_message"`), so it never clashes with the client's events or another mod's. |
| `...` | plain data | no | Values for the handlers, copied. |

**Returns** nothing.

**Errors** `mods.emit value <n> is not plain data` (with the path inside a table, e.g. `mods.emit value 1.fn`) when a
value holds a function or another value that cannot be copied; also for a number that is not finite, a string that is
not UTF-8 text, or tables nested more than 64 deep.

**Example**

```lua
-- Sender ("tally"):
events.on("battle_message", function(actor, target, message)
  mods.emit("tally_message", { actor = actor, target = target, message = message })
end)
-- Receiver (another mod):
events.on("tally_message", function(m)
  log.info("message " .. m.message .. " from " .. m.actor)
end)
```

Delivery is not immediate. The other mods get the event during a mod phase: the same frame when it is sent from a
command handler, a key bind or a HUD click, otherwise the next frame.
