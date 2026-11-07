# The modding guide

[Getting started](getting-started.md) builds a first mod step by step. This guide explains how mods work, one topic
at a time, each with code you can copy. For every function's details see the [reference](reference/README.md).

1. [What a mod is](#what-a-mod-is)
2. [When your code runs](#when-your-code-runs)
3. [Events](#events)
4. [Timing: frames, timers and waiting](#timing-frames-timers-and-waiting)
5. [Commands and typed lines](#commands-and-typed-lines)
6. [Reading the game](#reading-the-game)
7. [Acting in the game](#acting-in-the-game)
8. [Permissions](#permissions)
9. [Showing things](#showing-things)
10. [The chat log](#the-chat-log)
11. [Saving data](#saving-data)
12. [Packets](#packets)
13. [The network and other programs](#the-network-and-other-programs)
14. [Working with other mods](#working-with-other-mods)
15. [Mods that run before login](#mods-that-run-before-login)
16. [Limits and errors](#limits-and-errors)
17. [Conventions](#conventions)
18. [Tools for writing mods](#tools-for-writing-mods)
19. [Sharing your mod](#sharing-your-mod)

## What a mod is

A mod is a folder in the `mods` folder (`~/.local/share/vana-client/mods` on Linux, `%APPDATA%\vana-client\mods` on
Windows) with at least two files:

- **`mod.toml`**, which describes the mod.
- **`init.lua`**, the code that runs when the mod loads.

The folder's name is the mod's **id**: letters, digits, `_` and `-`. A mod runs when it is switched on in
Config → Settings → Mods.

### mod.toml

```toml
name = "Camp Helper"            # shown on the Mods and Extras pages
version = "1.2.0"               # your mod's own version
description = "Helps you camp." # one line
authors = ["You"]
permissions = ["actions"]       # what it may do; the player grants each (see Permissions)
```

Every key is optional:

| Key | What it is | Default |
| --- | --- | --- |
| `id` | The mod's id. | the folder's name |
| `name` | Its display name. | the id |
| `version` | Its version, shown on the Mods page. | `0.0.0` |
| `api` | The API version it was written for. A mod asking for a newer API than the client has is not loaded. | `1` |
| `authors` | A list of names. | none |
| `description` | One line about it. | none |
| `entry` | The file that runs at load. | `init.lua` |
| `permissions` | What it asks to do (see [Permissions](#permissions)). | none |
| `lobby` | `true` to run before login, from the login screen on (see [below](#mods-that-run-before-login)). | `false` |
| `[settings.<key>]` | Settings the player can change (see [Settings](#settings)). | none |

### More than one file

`require` loads another Lua file from your mod's folder, once, and returns what it returned:

```lua
-- util.lua
local M = {}
function M.percent(a, b) return math.floor(a * 100 / b) end
return M
```

```lua
-- init.lua
local util = require("util")        -- util.lua
local data = require("data.zones")  -- data/zones.lua, or data/zones/init.lua
chat.add(util.percent(3, 4) .. "%")
```

Each mod has its own Lua world: its globals, tables and functions are not shared with any other mod.

## When your code runs

| When | What runs |
| --- | --- |
| You log in, and your character is in the zone | Your `init.lua` from top to bottom, then the `load` event, then `login`. |
| While you play | Your event handlers, commands, timers and window handlers, as things happen. |
| You change zones | `zone_change`. |
| You log out | `logout`, then `unload`. Your storage is saved, and the mod is unloaded. |
| `/lua reload <id>` | `unload`, then the whole mod again from `init.lua`. |

The top level of `init.lua` is where you set things up: register events and commands, build windows, create HUD
elements. Your character's state is ready by then, so you can read it straight away (`game.player()` has your HP,
jobs and position).

```lua
local me = game.player()
chat.add(("Hello, %s. You are level %d."):format(me.name, me.main_level))

events.on("logout", function()
  log.info("Logging out; saving is automatic.")
end)
```

## Events

`events.on(name, handler)` calls `handler` every time the event happens, with the event's arguments. It returns an
id you can pass to `events.off` to stop.

```lua
local id = events.on("target_change", function(target_id)
  local t = target_id and game.entity(target_id)
  chat.add(t and ("Targeting " .. t.name) or "No target.")
end)

-- later:
events.off(id)
```

The kinds of events:

- **Changes to your character:** `hp_change`, `hpp_change`, `mp_change`, `tp_change`, `status_change`,
  `job_change`, `level_up`, `buff_gain`, `buff_lose`, `target_change`. Each gets the new value, and usually the old one.
- **The world:** `zone_change`, `weather_change`, `time_change`, `day_change`, `moon_change`, `party_change`.
- **Items:** `item_add`, `item_remove`, `item_change`, with the bag, the slot and the item.
- **Combat and chat:** `action` (anyone's action, decoded), `battle_message`, `damage`, `chat_message`.
- **The mod itself:** `load`, `unload`, `login`, `logout`, `settings_changed`, `frame`.

Every event and its arguments are in [reference/events.md](reference/events.md).

Handlers run in the order they were added. A handler with a higher priority runs before those with a lower one (the
default is 0): `events.on("action", handler, { priority = 10 })`.

## Timing: frames, timers and waiting

### Your code runs once a frame

The client runs your handlers once a frame, between reading the network and drawing. While they run, the game holds
still: everything you read describes the same moment.

**Changes you make take effect after your code finishes for that frame.** A target you set, a window you open, a
setting you change: they happen right after your handler returns. So reading a value back right after you set it gives
the old value:

```lua
actions.target(some_id)
print(game.target())   -- still the old target: the new one is set after this frame
```

To act on the result, wait a frame (`sleep_frames(1)`) or wait for the change (below).

### Timers

```lua
timer.after(5, function()
  chat.add("Five seconds later.")
end)

local id = timer.every(60, function()
  chat.add("Another minute.")
end)

timer.cancel(id)   -- stops it
```

### Waiting inside a handler

Every handler can pause itself without stopping the game: `sleep(seconds)`, `sleep_frames(n)`, and
`wait_until(test, timeout)`, which checks `test()` every frame until it returns true or `timeout` seconds pass. The
rest of the game, and your other handlers, keep running meanwhile.

```lua
commands.register("nearest", function()
  local best
  for _, e in ipairs(game.entities()) do
    if not e.me and e.targetable and (best == nil or e.distance < best.distance) then
      best = e
    end
  end
  if not best then return end

  actions.target(best.id)

  local function targeted()
    local t = game.target()
    return t ~= nil and t.id == best.id
  end
  wait_until(targeted, 2)    -- returns when targeted() is true, or after 2 seconds

  if targeted() then
    chat.add("Targeted " .. best.name .. ".")
  end
end)
```

`wait_until` returns nothing either way, so check the condition again afterwards, as here. This is the
[examples/waiting](examples/waiting/) mod.

A command that sleeps still counts as the player's input when it wakes, so it keeps the right to act (see
[Permissions](#permissions)).

### The frame event

`events.on("frame", function(dt) ... end)` runs every frame with the seconds since the last one. Use it only for
things that must follow the screen, like world drawing that tracks a moving target. For anything else, use events
or timers: code that runs every frame costs time every frame.

```lua
events.on("frame", function()
  local t = game.target()
  if t then
    draw.ring("ring", { at = t.pos, radius = 1.5, colour = { 255, 200, 60 } })
  else
    draw.clear()
  end
end)
```

## Commands and typed lines

```lua
commands.register("say_to", function(who, ...)
  local words = table.concat({ ... }, " ")
  commands.run(("/tell %s %s"):format(who, words))
end)
```

- Typing `/say_to Lnxtest hello there` calls the handler with `"Lnxtest"`, `"hello"`, `"there"`: the words after
  the command, split at spaces. `//say_to` works too.
- `commands.run(line)` runs a line exactly as if the player had typed it: a game command (`/tell`, `/ma`, `/equip`),
  your mod's commands, or another mod's. A game command is an action, so it needs a permission (see
  [Permissions](#permissions)). Running a `//` mod command does not.

`filters.typed` sees every line before the game reads it, and can change it or swallow it:

```lua
filters.typed(function(line)
  if line == "/gg" then
    return "/echo Good game, everyone!"   -- run this instead
  end
  -- returning nothing keeps the line as it was; returning false drops it
end)
```

## Reading the game

Reading needs no permission. The main places:

| To know | Use |
| --- | --- |
| Your character: name, HP / MP / TP, jobs, position, target | `game.player()` |
| Who and what is around: players, NPCs, monsters | `game.entities()`, `game.entity(id)`, `game.target()` |
| Your party and alliance | `game.party()` |
| Your items, by bag | `game.items(bag)`, `game.equipment()` |
| Status effects, recasts, spells and abilities known | `game.buffs()`, `game.recasts()`, `game.spells()`, `game.abilities()` |
| Zone, weather, Vana'diel time | `game.zone()`, `game.weather()`, `game.time()` |
| Names and data from the game's files | `res.item(id)`, `res.spell(id)`, `res.zone(id)`, `res.status(id)`... |

```lua
-- Every monster within 15 yalms, nearest first.
local mobs = {}
for _, e in ipairs(game.entities()) do
  if e.kind == "mob" and e.distance <= 15 and e.hpp > 0 then
    table.insert(mobs, e)
  end
end
table.sort(mobs, function(a, b) return a.distance < b.distance end)
for _, m in ipairs(mobs) do
  chat.add(("%s  %.1f yalms  %d%%"):format(m.name, m.distance, m.hpp))
end
```

The game's data (items, spells, abilities, statuses, zones) comes from `res`, which reads the game's files:

```lua
local item = res.item(4096)
chat.add(item.name)  -- "Fire Crystal"
```

The full lists are in [reference/game.md](reference/game.md) and [reference/res.md](reference/res.md); the tables
they return (Player, Entity, Member, Item...) are in [reference/types.md](reference/types.md).

## Acting in the game

Everything a player does has a function in `actions`: casting, abilities, weapon skills, items, equipment,
targeting, attacking, moving, trading, menus.

```lua
commands.register("cure", function()
  actions.cast(1, "<t>")      -- Cure (spell 1) on your target
end)

commands.register("food", function()
  actions.use_item(4422)      -- an item by id, from the inventory
end)
```

Leave the target out and the action picks the usual one: you for food, medicine and spells or abilities that only
target you, otherwise your current target.

Actions go through the same checks as the game's menus. A spell you don't know, or an ability on recast, is refused
with the game's usual message. Like every change, an action happens after your code finishes for the frame.

Acting needs a permission the player grants; read on.

## Permissions

A mod lists what it needs in `mod.toml`, and the player grants each one. The mod's Extras page shows a switch per
permission. Until a permission is granted, a call that needs it raises an error naming it.

```toml
permissions = ["actions", "net"]
```

| Permission | What it allows |
| --- | --- |
| *(none needed)* | Reading the game; your own windows, HUD and drawing; chat output; your commands; sounds; your settings and storage. |
| `actions` | Game actions while handling the player's input: in one of your commands, or a click or key in your window or HUD. |
| `automation` | Game actions at any time: from events, timers, or your own logic. |
| `packets.read` | Hooks that read packets. |
| `packets.write` | Changing, blocking and sending packets. |
| `input.capture` | Taking keys and the mouse away from the game, and pressing keys. |
| `files.shared` | A folder shared by every mod with this permission. |
| `net` | The internet and local connections: web requests, TCP, UDP, opening links. |
| `exec` | Starting programs, and recording the game window to video. |
| `multibox` | Talking to your other game windows on this computer, and moving or focusing the game window. |
| `ui.native` | Hiding the game's windows and HUD pieces, and changing game settings. |
| `clipboard` | Reading the clipboard (putting text on it needs nothing). |
| `process` | Which CPU cores the client runs on, and its priority. |
| `devices` | USB devices (G-keys, Stream Decks...) by their raw reports. |
| `mods` | Loading, unloading, reloading, hiding and showing other mods: `/lua load`, `unload`, `reload`, `hide` and `show` from `commands.run` or a key bind. |

**`actions` or `automation`?** A mod that helps the player act, like a button that casts a spell or a command that
equips a set, needs `actions`. A mod that acts by itself, like casting when HP drops or following someone, needs
`automation`. With only `actions`, an action from a timer or an event raises an error.

## Showing things

There are four ways to put something on screen. All work in both the classic and the modern interface.

| | What it is | Use it for |
| --- | --- | --- |
| `ui.window` | Windows with buttons, lists, sliders, text fields... drawn like the game's windows. | Anything the player works with. |
| `hud` | Text, boxes, bars, images and maps over the game. | Readouts that stay on screen. |
| `draw` | Lines, rings, boxes, text and images placed in the 3D world. | Marking places and things in the world. |
| `render` | How characters and the scene draw: hide, tint, resize, rename, post-process effects. | Changing what the game shows. |

### Windows

A window is a list of elements. You define it once and update it as things change; the client draws it and handles
the keys and the mouse.

```lua
ui.window("camp", {
  title = "Camp",
  width = 300,
  elements = {
    { type = "text", text = "Pick a spot:" },
    { type = "list", id = "spots", rows = { "North gate", "Lake", "Cave" }, height = 3 },
    { type = "slider", id = "range", label = "Range", steps = 10, at = 5 },
    { type = "buttons", id = "go", items = { "Go", "Cancel" } },
  },
})

ui.on("camp", function(_, e)
  if e.kind == "select" then
    chat.add("Spot " .. e.index)            -- the row picked, from 1
  elseif e.kind == "set" and e.id == "range" then
    chat.add("Range step " .. e.index)      -- the slider's step
  elseif e.kind == "press" and e.index == 2 then
    ui.close("camp")
  end
end)

commands.register("camp", function() ui.open("camp") end)
```

`ui.update(window, element, fields)` changes one element, for example `ui.update("camp", "spots", { rows = new_rows })`.
`ui.prompt` asks a question with buttons; `ui.menu` opens a popup list at the pointer:

```lua
ui.prompt("Leave the party?", { "Leave", "Stay" }, function(choice)
  if choice == 1 then commands.run("/pcmd leave") end
end)
```

Every element type, its fields, and every event a window sends are in [reference/ui.md](reference/ui.md).

### The HUD

```lua
hud.text("clock", { x = -10, y = 10, align = "right", text = "", colour = { 255, 230, 160 } })
timer.every(1, function()
  local t = game.time()
  hud.set("clock", { text = ("%02d:%02d"):format(t.hour, t.minute) })
end)
```

Positions are window pixels from the top left; negative ones count from the right or bottom edge. `hud.set` changes
some fields, `hud.remove` takes an element away, and `hud.on(id, handler)` takes its clicks and drags and tells you
when the pointer comes and goes. A `tip` shows a tooltip, and `anchor` makes an element's place count from one of the
game's panels, so it follows the panel. Icons can be an item's, a status effect's, a spell's or an ability's. See
[reference/hud.md](reference/hud.md).

### Drawing in the world

`draw` places shapes at world positions. They stay until you replace or remove them, and things standing in front of
them hide them, as they would hide a real object. [examples/target-ring](examples/target-ring/) draws a ring around
your target and its distance over its head. See [reference/draw.md](reference/draw.md).

### The game's windows

`ui.native` tells you which of the game's windows are open, what the cursor is on in them (an item, a spell), and the
rows of the list on screen. With the `ui.native` permission it can also hide HUD pieces: the party list, the target
bar, the compass and so on. See [reference/ui.native.md](reference/ui.native.md).

### Changing the game's own interface

A mod can change the built-in interface as well as add to it. In the modern UI, with the `ui.native` permission:

- **Hide** a panel (`ui.native.hide("focus")`) and draw your own in its place: HUD elements with `anchor = "focus"`
  stand where it was.
- **Move, scale or fade** a panel (`ui.native.place("target", {x = 600, y = 80, scale = 1.3, alpha = 0.9})`), and find
  where any panel is (`ui.native.rect`).
- **Reskin** it: replace its sprites with your PNGs, set its palette colours, or give its windows a new frame
  (`ui.skin.sprite`, `ui.skin.colour`, `ui.skin.frame`). Every panel, and every mod's window, draws with them.

All of it is undone when your mod unloads. In the classic UI the HUD pieces hide; its art is the game's own and is
replaced with a DAT overlay mod. See [reference/ui.native.md](reference/ui.native.md#modern-panels) and
[reference/ui.skin.md](reference/ui.skin.md).

## The chat log

```lua
chat.add("A plain line.")
chat.add("A line in the party chat's colour.", 5)   -- a chat mode: its colour, its tab and its filters
```

`filters.chat` sees every line before it reaches the log, and can keep it, drop it or rewrite it:

```lua
filters.chat(function(text, mode)
  if text:find("Synthesis interrupted", 1, true) then
    return false                     -- drop it
  end
  -- or return a changed line (and, optionally, a chat mode as a second value)
  return (text:gsub("%f[%w]gil%f[%W]", "Gil"))  -- the parentheses keep only gsub's first result
end)
```

A filter runs inside the client while the line passes, so it must be quick. It cannot `sleep` or read the game.
[examples/chat-tools](examples/chat-tools/) hides lines with a chosen word.

## Saving data

### Storage

`storage.account` (shared by your characters) and `storage.character` (this character only) are tables the client
saves for you, at logout, at unload, and when you call `storage.save()`:

```lua
storage.character.kills = (storage.character.kills or 0) + 1
storage.account.last_seen = os.time()
```

Store plain data: strings, numbers, booleans and tables of those. Keys come back as text, so index by
`tostring(id)` rather than by number.

To keep a secret, such as a password a mod uses to sign in, seal it: only your mod, on this computer, can open it.

```lua
storage.account.token = storage.seal("my secret")
local plain = storage.unseal(storage.account.token)
```

### Settings

Declare settings in `mod.toml`. The client draws them on your mod's page under Config → Extras, where the player
changes them:

```toml
[settings.sound]
type = "bool"
default = true
label = "Sound"
help = "Play a sound on warnings."

[settings.style]
type = "enum"
default = "Small"
options = ["Small", "Large"]
label = "Style"
```

```lua
if settings.get("sound") then audio.play_ui(4) end   -- the game's "not allowed" sound

events.on("settings_changed", function(key, value)
  if key == "style" then rebuild() end
end)
```

| `type` | Value | Extra keys |
| --- | --- | --- |
| `bool` | true / false | |
| `int` | a whole number | `min`, `max`, `step` |
| `number` | a number | `min`, `max`, `step` |
| `enum` | the chosen option's label (a string) | `options`: the labels; `default` is one of them |
| `string`, `colour`, `key` | text, `{r, g, b, a}`, a key name | |

Each setting can also have `label`, `help`, and `scope`: `"account"` (the default) or `"character"`, for a value per
character. The Extras page draws `bool`, `enum`, `int` and `number` settings; the others can be set with
`/lua set <mod> <key> <value>`.

### Files and JSON

```lua
fs.write("notes.txt", "remember the cave\n")
fs.append("notes.txt", "and the lake\n")
local text = fs.read("notes.txt")

local data = json.decode(fs.read("data.json") or "{}")
fs.write("data.json", json.encode(data))
```

Files go in your mod's own data folder. `fs.read` also finds files shipped in your mod's folder, so data you
include with the mod can be read the same way. See [reference/fs.md](reference/fs.md).

## Packets

With `packets.read`, a mod can watch the packets the server and the client exchange. With `packets.write`, it can
also change, block and send them:

```lua
packets.on_in(0x00A, function(p)
  log.info(("zone-in packet, %d bytes"):format(#p.data))
end)
```

`p.data` is the whole packet as a string of bytes, header included. Read numbers out of it with Lua's
`string.unpack`. The handler can return changed bytes to replace the packet, or `false` to block it. See
[reference/packets.md](reference/packets.md).

## The network and other programs

With the `net` permission:

```lua
net.request({ url = "https://example.com/prices.json" }, function(res)
  if res.error then
    chat.add("Request failed: " .. res.error)
  else
    local prices = json.decode(res.body)
  end
end)
```

`net.connect`, `net.listen` and `net.udp` talk TCP and UDP, and `net.pipe` opens a local pipe or socket another
program offers. Results always arrive in a later frame, in your callbacks. `client.run` starts a program (the `exec`
permission). See [reference/net.md](reference/net.md) and [reference/client.md](reference/client.md).

## Working with other mods

```lua
-- In the mod "tracker":
mods.provide("count", function(name) return counts[name] or 0 end)

-- In another mod:
local n = mods.call("tracker", "count", "Wild Rabbit")
```

`mods.emit("tracker_found", name)` raises an event in every other mod, which they watch with `events.on`. Name your
events after your mod so they don't clash. Values cross between mods as copies of plain data.

To talk to the same mod in your other game windows on this computer, use `ipc.send` and the `ipc_message` event
(the `multibox` permission).

## Mods that run before login

`lobby = true` in mod.toml makes a mod start on the login screen and stay loaded until the client closes. Before
login it sees the lobby through `lobby.state()` and the `lobby_screen` event, and can sign in and pick a character
with `lobby.sign_in` and `lobby.play`. There is no game to read there: game calls, and calls that act in the world
(actions, sounds, the clipboard, the window...), raise an error, and its prints go to `vana-client.log`. `ipc`
messages work there too. Once you are in the world it works like any other mod, and at logout it stays loaded for the
next login. See [reference/lobby.md](reference/lobby.md).

## Limits and errors

| Limit | What happens |
| --- | --- |
| A handler runs longer than 100 ms without pausing | It is stopped, with an error. |
| A filter, packet hook or `mods.call` runs longer than 20 ms | It is stopped, with an error. |
| The mod uses more than 64 MB | The allocation fails with an out-of-memory error. |
| 20 errors within 60 seconds | The mod is suspended: its handlers, commands, filters and offered functions stop and its windows close until you fix it and `/lua reload` it. |

An error shows once in the chat log, in red, with your mod's name and where it happened (`init.lua:24: ...`). The full
message goes to `vana-client.log` in the client's data folder. One failing handler doesn't stop the others.

Long work should be split up: process part of a list, `sleep_frames(1)`, then carry on.

## Conventions

- **Positions** are `{x, y, z}` in yalms. `x` and `z` are the ground; `y` is the height, and **y points down**: a
  smaller `y` is higher. To draw above someone's head, subtract from their `y`.
- **Headings** are radians, 0 to 2π.
- **Ids and indexes:** each entity has an `id` (its server id, unique) and an `index` (its slot in the zone).
  Functions that take an entity take its `id`.
- **Counting starts at 1** for list rows, buttons, slots in Lua tables, and the like, as in Lua itself. Numbers that
  come from the game keep the game's values: bag 0 is the inventory; job, status, item and spell ids are the game's.
- **Colours** are `{r, g, b, a}`, each 0 to 255; `a` (opacity) defaults to 255. `{ r = 255, g = 0, b = 0 }` works too.
- **Time** is in seconds.
- **No value** is `nil`. Optional arguments can be left out.

## Tools for writing mods

**Editor support.** When you log in, the client writes a description of the whole API into the mods folder
(`.library/vana.lua` and `.luarc.json`). Open the mods folder in an editor with a Lua language server (VS Code with
the "Lua" extension, for example) and you get completion, argument hints and checks for every API function.

**The `/lua` command:**

| Command | Does |
| --- | --- |
| `/lua list` | The loaded mods, whether each is running, their time per frame and memory. |
| `/lua load <id>` / `/lua unload <id>` | Loads or unloads a mod. |
| `/lua reload <id>` | Reloads a mod (alone: reloads every mod). |
| `/lua watch <id>` | Reloads the mod whenever you save one of its files. Run it again to stop. |
| `/lua run <id> <code>` | Runs a line of Lua inside a mod: `/lua run hello chat.add(game.player().name)`. |
| `/lua set <id> <key> <value>` | Changes a setting. |
| `/lua settings <id>` | Opens the mod's Extras page. |
| `/lua grant <id> <permission>` / `/lua revoke ...` | Grants or takes away a permission. |
| `/lua hide` / `/lua show` | Hides or shows every mod's windows and drawing. |

**F12** shows each mod's time per frame, its slowest call, its memory and its errors.

**`log.info`, `log.warn`, `log.error`** write to `vana-client.log` without cluttering the chat log.

## Sharing your mod

A mod is its folder: zip it (with the folder inside the zip) and share the file. To install one, unzip it into the
mods folder. It shows up on the Mods page switched off; switch it on, and grant the permissions it asks for on its
Extras page.

Put a `README.md` in the folder saying what the mod does, its commands, and which permissions it asks for and why.
