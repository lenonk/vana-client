# vana-client mod API

Mods are Lua 5.4 programs the client runs beside the game. They read the game's state, react to its events, draw their
own windows and HUD in the client's look, and act in the game where the player allows it. This document is the API's
full description: a guide to how mods work, then the reference of every function, event and type, generated from the
client's own table of them so it always matches the build.

The API version is **1** (`api` in mod.toml). The design notes and the state of the work are in
[lua-mods.md](lua-mods.md); which Windower and Ashita add-ons the API can carry is in [lua-coverage.md](lua-coverage.md).

## Contents

1. [A first mod](#1-a-first-mod)
2. [The package](#2-the-package)
3. [Lifetime](#3-lifetime)
4. [How a mod runs](#4-how-a-mod-runs)
5. [The Lua environment](#5-the-lua-environment)
6. [Permissions](#6-permissions)
7. [Conventions](#7-conventions)
8. [Windows, HUD and drawing](#8-windows-hud-and-drawing)
9. [Data: storage, settings, files](#9-data-storage-settings-files)
10. [Commands](#10-commands)
11. [Writing and debugging](#11-writing-and-debugging)
12. [Reference](#reference)

## 1. A first mod

A folder in the mods folder, with a manifest and an entry file:

```
mods/hello/
  mod.toml
  init.lua
```

`mod.toml`:

```toml
name = "Hello"
version = "1.0.0"
description = "Greets the zone."
```

`init.lua`:

```lua
events.on("zone_change", function(zone)
  chat.add("Welcome to " .. (res.zone(zone) or "somewhere") .. ".")
end)

commands.register("hello", function(who)
  chat.add("Hello, " .. (who or game.player().name) .. "!")
end)
```

Turn it on in Settings > Mods (the mods folder lists it), then `/lua reload` in game, or log in again. `/hello` and
`//hello` now run it. `/lua watch hello` reloads it whenever its files are saved, so it can be written while playing.

## 2. The package

**Where:** `mods/<id>/` in the client's data folder (`~/.local/share/vana-client/mods` on Linux), beside the DAT
overlay packs; one folder can carry both. A mod runs when it is enabled on the Mods page (config `mods`). Mods built
into the client (bundled) run unless turned off there, or replaced by a folder of the same id.

**mod.toml:**

| Key | Meaning | Default |
| --- | --- | --- |
| `id` | Letters, digits, `_` and `-`. | the folder's name |
| `name` | Shown on the Mods and Extras pages. | the id |
| `version` | The mod's own version (semver is the convention). | `0.0.0` |
| `api` | The API version it is written for; a mod asking for a newer one than the client has is not loaded. | `1` |
| `authors` | A list of names. | none |
| `description` | One line. | none |
| `entry` | The file run at load. | `init.lua` |
| `permissions` | What it asks for (see [Permissions](#6-permissions)); the player grants each. | none |
| `lobby` | `true`: it runs from the login screen to the end of the process, not from login to logout (see [Lifetime](#3-lifetime)). | `false` |
| `[settings.<key>]` | A setting the client shows on the mod's Extras page (see [Settings](#9-data-storage-settings-files)). | none |

A setting declares `type` (`bool`, `int`, `number`, `enum`, `string`, `colour` or `key`), `default`, `label`, `help`,
`scope` (`account`, the default, or `character`), `min`, `max` and `step` for numbers, and `options` (the labels) for
an enum. The Extras page draws the `bool`, `enum`, `int` and `number` ones; every type can be read with `settings.get`
and set with `/lua set`.

```toml
[settings.threshold]
type = "int"
default = 75
min = 0
max = 100
step = 5
label = "Warn below"
help = "Warn when HP drops below this percent."
```

## 3. Lifetime

- **Load:** at character login, once the first zone is in. The entry file runs, then `load`, then `login(name)`.
- **In the world:** events arrive each frame (`zone_change`, `frame`, the derived change events, packets...).
- **Logout:** `logout`, then `unload`; storage is saved and the VM is dropped.
- **Lobby mods** (`lobby = true`): loaded on the login screen and kept until the client closes. In the lobby they get
  `lobby_screen` and `frame`, read `lobby.state` and act with `lobby.sign_in` / `lobby.play`; there is no game state
  there (game calls raise an error) and what they print goes to vana-client.log. At each login they get
  `storage.character` and their character settings and the world's events; at logout `logout`, but they stay loaded.
  They get `unload` as the client closes.
- **Reload:** `/lua reload <mod>` (or `/lua watch <mod>`) unloads and loads it again in place; storage is saved first.
- **Suspension:** 20 errors within 60 seconds suspend a mod (a call stopped by the time budget, or one that runs out
  of memory, is an error like any other): its handlers stop, its windows hide, its storage is kept, and a line says
  why. `/lua reload <mod>` starts it again.

## 4. How a mod runs

**The mod phase.** Once a frame, after the network and before drawing, the client runs every mod's due work: events,
timers, sleeping handlers waking, socket and device data, window and HUD clicks, then `frame`. During the phase the
game's state is lent to the mods: every read sees this frame's state.

**Writes are queued.** Anything that changes the game (an action, a chat line, a switch, a window request) is queued
and applied after the phase, through the same paths the keys and the menus take. A value read back right after it is
set therefore shows the old value until the next frame.

**Handlers are coroutines.** Every handler (events, commands, timers, window events) runs as a coroutine and may pause
with `sleep(seconds)`, `sleep_frames(n)` or `wait_until(test, timeout)`; the game and the other handlers go on
meanwhile. A sleeping handler wakes in a later phase.

**Inline callbacks.** Filters (`filters.chat`, `filters.typed`), packet hooks and functions other mods call
(`mods.call`) run inside the client's own paths, when the line, the packet or the call comes. They cannot sleep, and
filters cannot read game state; each call has a 20 ms budget.

**Budgets and limits:**

| Limit | Value | What happens |
| --- | --- | --- |
| One call (a handler run until it returns or sleeps) | 100 ms | the call is stopped with "ran past its time budget" (an error) |
| A slow call | over 2 ms | counted as slow on the F12 lines |
| An inline callback (filter, packet hook, `mods.call`) | 20 ms | stopped, as above |
| Memory | 64 MB a mod | the allocation fails with an out-of-memory error |
| Errors | 20 within 60 s | the mod is suspended |

Each mod has its own Lua state: mods share nothing but what they pass through `mods.provide` / `mods.call` /
`mods.emit` (copies of plain data).

## 5. The Lua environment

Lua 5.4 with `table`, `string`, `math`, `utf8`, `coroutine` and the base library, and `os.time`, `os.date`,
`os.clock` and `os.difftime`. Not there: `io`, `package`, `debug`, the rest of `os`, `dofile`, `loadfile` and
`string.dump`. `load` takes text only (no bytecode); `collectgarbage("count")` gives the mod's memory in KB and every
other option does nothing.

- `require("a.b")` loads `a/b.lua` or `a/b/init.lua` from the mod's folder, once.
- `print(...)` writes to the chat log (dim) and to vana-client.log; `log.*` to the log file only.
- Data from the client that has no value is `nil` (a JSON null becomes nil everywhere, `json.decode` included).
- Game text is UTF-8. Two kinds of special characters can sit in it: auto-translate phrases (`text.phrase`,
  `text.autotranslate`; `text.plain` turns them into words) and chat icons (`chat.icon`).

## 6. Permissions

A mod lists what it needs in `permissions`; the player grants each on the mod's Extras page (`/lua settings <mod>`)
or with `/lua grant <mod> <permission>`, and can revoke it the same way. Grants are kept in `mod-data/grants.json`. A
call that needs an ungranted permission raises an error naming it.

| Permission | Allows |
| --- | --- |
| *(none)* | Reading state; its own windows, HUD, drawing, settings and storage; chat output; its own commands; sounds. |
| `actions` | Game actions (casting, items, equipment, targeting, moving, menus) while handling the player's input: one of its commands, a click or key in its windows. |
| `automation` | Game actions on its own: from events, timers and its own logic. |
| `packets.read` | Packet hooks that read. |
| `packets.write` | Changing, blocking and injecting packets. |
| `input.capture` | Taking keys and the mouse from the game, and pressing keys. |
| `files.shared` | Files in the shared area (`shared/...`) every such mod can reach. |
| `net` | The internet and local sockets: HTTP, TCP, UDP, pipes, opening links. |
| `exec` | Starting programs (`client.run`), recording the window (`client.record`). |
| `multibox` | Messages to the other game windows on this computer, and the game window's place, size and focus. |
| `ui.native` | Hiding and showing the game's own windows and HUD pieces, setting the game's config. |
| `clipboard` | Reading the clipboard (writing it needs nothing). |
| `process` | The client's CPU affinity and priority. |
| `devices` | USB HID devices by their raw reports. |

Never reachable by any mod: the password and the two-factor secret, the client's `secret.key`, session keys and the
network encryption.

## 7. Conventions

- **Places** are world coordinates in yalms, as the server sends them: `x` and `z` on the ground, `y` the height with
  **y pointing down** (a smaller y is higher). Distances are yalms.
- **Headings** are radians, 0 to 2π.
- **Ids:** an entity's `id` is its server id (players below 0x1000000; NPCs and monsters above, by zone); its `index`
  is the zone's target index. Functions that take an entity take its server id unless they say otherwise.
- **Counting from 1:** list rows, window elements, choices, slots and the like are numbered from 1, as Lua's own
  tables are. Raw game values keep their own numbering: bag numbers (0 the inventory), equipment slots, job and
  status ids, chat modes.
- **Colours** are `{r, g, b, a}` from 0 to 255 (`{r = , g = , b = , a = }` works too; `a` defaults to 255).
- **Chat modes** are the game's own (0..255): a mode picks a line's colour, its log tab and its filters.
- **Time:** delays and durations are seconds; `clock()` is seconds since the mod loaded.
- **Optional arguments** end in `?` in the reference (`sleep(seconds?)`); a function's result that may be missing has
  a type ending in `?`.

## 8. Windows, HUD and drawing

There are four ways to show things, each in both UIs:

- **`ui.window`:** windows built from elements (text, buttons, sliders, lists with columns and icons, gauges, tabs,
  choices, colour pickers, text input, sections, canvases). The client draws them in the active UI's own look: a
  Config page in the classic UI, kit panels in the modern UI, with their keys and mouse. `ui.on(id, handler)` gets
  what the player did: `press`, `set`, `select`, `edit` and `change` (typing, a colour), `toggle`, `sort`, `click`,
  `drag_start` / `drag` / `drag_end` on a canvas, `close`. `ui.prompt` and `ui.menu` give a question and a popup list.
- **`hud`:** text, rects, bars, images and maps over the scene, under the game's windows, placed in UI units
  (`hud.screen` gives the window in pixels and the pixels per UI unit).
- **`draw`:** lines, rings, boxes, text and images placed in the world, depth-tested against the scene.
- **`render`:** how entities draw (hidden, name, colours, size), the camera, and WGSL post-process passes.

`ui.native` reads and drives the game's own windows (what is open, the highlighted row, the open list) and hides HUD
pieces; `/lua hide` hides every mod's drawing at once.

## 9. Data: storage, settings, files

- **`storage.account`** and **`storage.character`** are plain tables (strings, numbers, booleans, tables) the client
  saves for the mod: per account, and per character. They are written at logout, unload and `storage.save()`.
  `storage.seal` / `storage.unseal` keep a secret (a password) that only this mod on this machine can open.
- **Settings** declared in mod.toml are read with `settings.get(key)` (the player's value, else the default); a change
  on the Extras page or by `/lua set` raises `settings_changed(key, value)`.
- **Files:** `fs.*` work in the mod's data folder (`mod-data/<id>/files`); reads fall back to the mod's own files, so
  data shipped in the package can be read with `fs.read`. With `files.shared`, `shared/...` is a folder every such mod
  reaches.
- **JSON:** `json.encode` / `json.decode`.

Everything a mod keeps is under `mod-data/<id>/` beside the mods folder: `account.json`, `character-<world>-<name>.json`,
`settings.json`, `settings-character-<world>-<name>.json` and `files/`.

## 10. Commands

A registered command is typed as `/name` or `//name`; its handler gets the words after it. Lines starting `//` always
go to the mods. `/lua` manages them:

| Command | Does |
| --- | --- |
| `/lua list` | The loaded mods, their state, time a frame and memory. |
| `/lua load <mod>` / `unload <mod>` | Loads or unloads one. |
| `/lua reload [<mod>]` | Reloads one, or every loaded mod (and loads one that failed). |
| `/lua watch <mod>` | Reloads it whenever its files are saved (on or off; kept across sessions). |
| `/lua run <mod> <code>` | Runs Lua in the mod's state, as its own input. |
| `/lua set <mod> <key> <value>` | Sets a setting (JSON or text). |
| `/lua settings [<mod>]` | Opens the mod's Extras page. |
| `/lua grant` / `revoke <mod> <permission...>` | Grants or revokes permissions. |
| `/lua hide` / `show` | Hides or shows every mod's HUD, windows and world drawing. |

## 11. Writing and debugging

- **Editor support:** at each login the client writes LuaLS definitions of the whole API into the mods folder
  (`.library/vana.lua` and `.luarc.json`). Open the mods folder in an editor with the Lua language server for
  completion and checks.
- **Errors** show once each in the chat log in red with the mod's name and where (`file:line`); the full text goes to
  `vana-client.log` (in the client's data folder).
- **F12** shows each mod's time a frame, its peak, slow calls, memory and errors.
- `/lua run <mod> <code>` tries a line in a running mod; `/lua watch` keeps the mod reloading as you edit.

## Reference

<!-- Reference: generated by `vana-client lua-docs` from crates/vana-client/src/client/lua/docs.rs. Edit that, not this part. -->

### Namespaces

| Namespace | Functions | What it is |
| --- | --: | --- |
| [Globals](#globals) | 6 | Functions every mod has: printing, modules, waiting, the clock. |
| [`log`](#log) | 4 | Lines in vana-client.log. |
| [`events`](#events) | 2 | Handlers for the client's events (see the event list). |
| [`commands`](#commands) | 2 | The mod's own text commands, and running the game's. |
| [`filters`](#filters) | 3 | Chat lines and typed lines rewritten or dropped before the client takes them. |
| [`timers`](#timers) | 3 | Rows in the client's Timers window. |
| [`ui`](#ui) | 10 | The mod's windows, in the active UI's own look. |
| [`hud`](#hud) | 12 | Text, bars, images and maps drawn over the game, under the game's windows. |
| [`ui.native`](#uinative) | 11 | The game's own windows and HUD pieces: what is open, what is highlighted, hiding, the config. |
| [`render`](#render) | 8 | How entities and the camera draw, and post-process passes. |
| [`audio`](#audio) | 5 | The game's sounds and music, and the mod's own. |
| [`draw`](#draw) | 8 | Lines, shapes, text and images placed in the world. |
| [`net`](#net) | 8 | HTTP, TCP, UDP, local pipes and sockets, and links (the net permission). |
| [`ipc`](#ipc) | 1 | Messages between game windows on this computer (multibox). |
| [`packets`](#packets) | 7 | Hooks on the packets the server and the client send, and injecting them. |
| [`chat`](#chat) | 4 | The chat log and the chat line. |
| [`input`](#input) | 9 | The keys and the mouse: watching, taking and pressing them. |
| [`binds`](#binds) | 3 | Key binds that run lines, as the game's own binds do. |
| [`timer`](#timer) | 3 | Calls after a delay or at an interval. |
| [`game`](#game) | 38 | The game's state: our character, the zone, entities, party, items, jobs and more. |
| [`actions`](#actions) | 51 | Doing things in the game: casting, items, equipment, targeting, moving, menus (actions or automation). |
| [`res`](#res) | 24 | Game data from the DATs: items, spells, abilities, statuses, zones, messages, names. |
| [`fs`](#fs) | 8 | Files in the mod's data folder (and its own files, read only). |
| [`json`](#json) | 2 | JSON text to Lua tables and back. |
| [`mods`](#mods) | 5 | The other mods: the list, calls into them, events to them. |
| [`text`](#text) | 5 | Shift-JIS and auto-translate phrases in strings. |
| [`client`](#client) | 15 | The client itself: the window, the clipboard, screenshots, recording, programs, the process, switches. |
| [`devices`](#devices) | 4 | USB HID devices by their raw reports (the devices permission). |
| [`storage`](#storage) | 3 | The mod's saved data (storage.account, storage.character) and sealed secrets. |
| [`lobby`](#lobby) | 3 | The screens before login, for lobby mods. |
| [`settings`](#settings) | 1 | The settings declared in mod.toml. |
| [Event list](#event-list) | 37 | What `events.on` can watch. |
| [Types](#types) | 11 | The tables the API hands out and takes. |

### Globals

Functions every mod has: printing, modules, waiting, the clock.

#### `print(...)`

Writes its arguments to the chat log (dim) and vana-client.log.

- `...` `any`: values, tab-separated

#### `require(name)`

Loads a Lua file from the mod's folder once (a.b is a/b.lua or a/b/init.lua) and returns what it returned.

- `name` `string`: module name

Returns `any`.

#### `sleep(seconds?)`

Pauses the running handler for `seconds` (other handlers and the game go on).

- `seconds?` `number`: default 0: the next frame

#### `sleep_frames(n?)`

Pauses the running handler for `n` frames.

- `n?` `integer`: default 1

#### `wait_until(test, timeout?)`

Pauses the running handler until `test()` returns true (checked once a frame), or `timeout` seconds pass.

- `test` `fun(): boolean`: the condition
- `timeout?` `number`: seconds; none waits for ever

#### `clock()`

Seconds since the mod was loaded (monotonic).

Returns `number`.

### `log`

Lines in vana-client.log.

#### `log.info(text)`

Writes a line to vana-client.log.

- `text` `string`

#### `log.warn(text)`

Writes a warning to vana-client.log.

- `text` `string`

#### `log.error(text)`

Writes an error to vana-client.log.

- `text` `string`

#### `log.debug(text)`

Writes a debug line to vana-client.log (shown with debug logging on).

- `text` `string`

### `events`

Handlers for the client's events (see the event list).

#### `events.on(event, handler, options?)`

Calls `handler` on every `event` (see the events list), as a coroutine that may sleep. Higher priority runs first.

- `event` `vana.Event|string`: event name
- `handler` `function`: its arguments are the event's
- `options?` `{priority: integer?}`

Returns `integer`.

#### `events.off(id)`

Removes a handler added by events.on.

- `id` `integer`: what events.on returned

### `commands`

The mod's own text commands, and running the game's.

#### `commands.register(name, handler)`

Adds a command: typed as //name or /name, it calls `handler` with the words after it.

- `name` `string`: without the slash
- `handler` `fun(...: string)`

#### `commands.run(line)`

Runs a line as if the player had typed it (game commands, our commands, other mods' commands). Any line but a //mod command is a game action: it needs actions (while handling the player's input) or automation.

- `line` `string`: e.g. '/ma "Cure" <t>'

### `filters`

Chat lines and typed lines rewritten or dropped before the client takes them.

#### `filters.chat(filter)`

Runs `filter` on every chat line before it is kept: return nil to keep it, false to drop it, or new text (and optionally a new mode) to replace it. Runs inline: it cannot sleep or read game state.

- `filter` `fun(text: string, mode: integer?, battle: boolean): (string|false|nil, integer?)`

Returns `integer`.

#### `filters.typed(filter)`

Runs `filter` on every line typed or run (macros, mods) before the command parser: nil keeps it, false drops it, a string replaces it. Runs inline.

- `filter` `fun(line: string): string|false|nil`

Returns `integer`.

#### `filters.off(id)`

Removes a filter.

- `id` `integer`: what filters.chat / filters.typed returned

### `timers`

Rows in the client's Timers window.

#### `timers.add(name, seconds, options?)`

Adds or replaces a row in the Timers window (shown for mods' rows even with the Timers Extra off), counting down `seconds`: options section ('recast' | 'buff' (default) | 'debuff'), group (a heading, e.g. a target's name), icon {kind = 'status' | 'spell' | 'ability', id}, total (the gauge's whole length), fills (toward ready, as recasts do).

- `name` `string`
- `seconds` `number`
- `options?` `table`

#### `timers.remove(name)`

Removes the mod's row by name.

- `name` `string`

#### `timers.clear()`

Removes every row of the mod's.

### `ui`

The mod's windows, in the active UI's own look.

#### `ui.window(id, spec)`

Creates or replaces a window: title, width, help, and elements in order. Element types: text {id, text, dim, colour}; buttons {id, label, items, marked} (marked makes them options); slider {id, label, steps, at, shown}; list {id, rows, columns, at, height} (a row is a string or {cells..., icon = {kind = 'item' | 'status', id}, colour}; columns {title, width, align = 'right'}, a heading click sends 'sort'); gauge {id, label, value 0..1, text, kind hp|mp|tp|cast|experience|recast}; separator; tabs {id, items, at} (pages: an element's page = n shows only on page n of the tabs above it); input {id, label, text, max} (a typed line: Enter starts and ends it, Escape undoes it); choice {id, label, items, at} (Left / Right turn it); colour {id, label, value = {r, g, b, a}, alpha} (Enter edits it a channel at a time); section {id, label, open} (folds what is under it, to the next section or tabs); canvas {id, w, h, draw} (a region the mod draws in, in its own units: draw is a list of {type = 'line', x1, y1, x2, y2, width} | {type = 'rect', x, y, w, h, to (a bottom colour: a gradient), line (outline width)} | {type = 'circle', x, y, r, line} | {type = 'tri', x1, y1, x2, y2, x3, y3} | {type = 'text', x, y, text} | {type = 'icon', x, y, size, kind = 'item' | 'status', id}, each with a colour; ui.update(window, canvas, {draw = ...}) redraws it; a click sends 'click' {x, y}, a drag 'drag_start', 'drag' and 'drag_end' {x, y, dx, dy}, held to the release wherever the pointer goes). Every element may have tip (the help shown while the cursor or pointer is on it) and page. Indexes are 1-based. Window options: hud (never takes the keys), frame = false (no frame), x / y (its place in the classic UI), modal (keeps the keys and the pointer while open; centred unless placed), popup (modal, opened at the pointer, closed by a click outside it). It draws in the active UI's look (classic Config page or modern panel).

- `id` `string`
- `spec` `vana.WindowSpec`

#### `ui.update(id, element, fields)`

Changes one element of a window (only the fields given): e.g. a list's rows, a text's text, a slider's at and shown.

- `id` `string`: window
- `element` `string`: element id
- `fields` `table`

#### `ui.prompt(text, buttons?, handler)`

A modal question in the UI's own look: the text and buttons (default Yes / No); the handler gets the button's 1-based index, or nil when Escape closes it.

- `text` `string`
- `buttons?` `string[]`
- `handler` `fun(index: integer?)`

#### `ui.menu(items, handler)`

A popup list at the pointer (a right-click menu): the handler gets the row's 1-based index, or nil when it is closed (Escape, a click outside it).

- `items` `string[]`
- `handler` `fun(index: integer?)`

#### `ui.get(id, element)`

An element's value as the player left it: {text} (text, input), {at} (buttons with options, slider, list, tabs, choice), {value} (gauge; a colour's {r, g, b, a}), {open} (section).

- `id` `string`: window
- `element` `string`: element id

Returns `table`.

#### `ui.open(id)`

Shows a window and gives it the keys (arrows, Enter, Escape); the mouse works in both UIs.

- `id` `string`

#### `ui.show(id)`

Shows a window without giving it the keys (a HUD: hud = true windows never take them; a click still acts).

- `id` `string`

#### `ui.close(id)`

Hides a window.

- `id` `string`

#### `ui.is_open(id)`

Whether a window shows.

- `id` `string`

Returns `boolean`.

#### `ui.on(id, handler)`

The window's handler: called with the window id and an event {kind, id = element id, ...}: 'press' / 'set' / 'select' {index, 1-based} (buttons, options, slider, list, tabs, choice), 'edit' {text} as a line is typed and 'change' {text} when Enter ends it, 'change' {value = {r, g, b, a}} (colour), 'toggle' {open} (section), 'sort' {column} (a list heading), 'click' {x, y} (a canvas), 'close'.

- `id` `string`
- `handler` `fun(window: string, event: {kind: string, id: string?, index: integer?, text: string?, value: table?, open: boolean?, column: integer?, x: number?, y: number?})`

### `hud`

Text, bars, images and maps drawn over the game, under the game's windows.

#### `hud.text(id, spec)`

Creates or replaces a HUD text: x, y (window pixels; negative from the right or bottom edge), text (\n for lines), size (1 = the UI's text), colour and bg ({r, g, b, a} 0..255), align (left, center, right), pad, layer, visible, font ('game' the game's font, 'regular' / 'bold' the modern faces, in either UI; else the active UI's), outline ({r, g, b, a}) and outline_width, shadow. It draws under the windows and hides with the UI.

- `id` `string`
- `spec` `table`

#### `hud.rect(id, spec)`

Creates or replaces a filled rectangle: x, y, w, h (window pixels; hud.screen().ui_scale converts UI units), colour.

- `id` `string`
- `spec` `table`

#### `hud.bar(id, spec)`

Creates or replaces a bar: x, y, w, h, value (0..1), kind (hp, mp, tp, cast, experience, recast: its colour) or colour.

- `id` `string`
- `spec` `table`

#### `hud.map(id, spec)`

Creates or replaces a HUD map: the zone's map where the client shows one, x, y, w, h, centre {x, z} (world; default: us), span (map pixels across, 16..2048; default 256), alpha. Draw markers over it with other elements at game.map_pos.

- `id` `string`
- `spec` `table`

#### `hud.image(id, spec)`

Creates or replaces an image from a PNG in the mod's folder: x, y, path, w and h (window pixels; default the image's size), colour (a tint).

- `id` `string`
- `spec` `table`

#### `hud.icon(id, spec)`

Creates or replaces an icon: x, y, icon (status or item), id, size.

- `id` `string`
- `spec` `table`

#### `hud.set(id, fields)`

Changes an element's fields (only those given).

- `id` `string`
- `fields` `table`

#### `hud.remove(id)`

Removes an element.

- `id` `string`

#### `hud.clear()`

Removes every element of the mod.

#### `hud.on(id, handler)`

An element's click handler: a left or right press on it is the mod's (not the game's): handler(id, {kind = 'click', id, button}). It is the player's input: actions may act here.

- `id` `string`
- `handler` `fun(id: string, e: table)`

#### `hud.screen()`

The window in pixels, and ui_scale: pixels per UI unit (sizes given in UI units look the same at any resolution).

Returns `{width: number, height: number, ui_scale: number}`.

#### `hud.bounds(id)`

Where an element was drawn last frame, in window pixels.

- `id` `string`

Returns `{x: number, y: number, w: number, h: number}?`.

### `ui.native`

The game's own windows and HUD pieces: what is open, what is highlighted, hiding, the config.

#### `ui.native.windows()`

The game's windows open now, by name (menu, items, magic, ability, map, config, status, shop, trade, event, choice...).

Returns `string[]`.

#### `ui.native.highlighted()`

What the cursor is on in the game window with the keys: {window, kind (item, spell, ability (its name-table id), mount, emote, key_item), id, bag and slot for an item in a bag}; nil when none.

Returns `{window: string, kind: string, id: integer, bag: integer?, slot: integer?}?`.

#### `ui.native.list()`

The game list with the keys: window (items, magic, ability, keyitems, shop buy / sell / wanted / bazaar / wares, auction list / history / sell or an auction menu's name), rows (each its item, spell, ability, mount, emote or key_item id, and count, price, bag, slot, equipped, stack counts where the list has them), cursor and top (the first row shown), both from 1. An auction menu has no rows; its cursor is the row's child id. Drive it with input.press.

Returns `{window: string, rows: table[], cursor: integer, top: integer}?`.

#### `ui.native.menu_open()`

Whether any game window has the keys (a HUD may hide meanwhile).

Returns `boolean`.

#### `ui.native.interface_hidden()`

Whether the interface is hidden (the UI-hide key).

Returns `boolean`.

#### `ui.native.interface(show)`

Shows or hides the whole interface, as the UI-hide key does. Needs ui.native.

- `show` `boolean`

#### `ui.native.hide(piece, hidden?)`

Hides a HUD piece of the game in both UIs while the mod is loaded: party (party and alliance lists, their status icons), target, buffs (our status icons), compass (the modern location panel), minimap, cast (the cast bar), plates (name plates), alliance (the alliance lists alone), log (the log windows: back while an event, a prompt or the chat line needs them), timers, regime (the training regime window), network (the connection panel), palette (the macro palette's drawing; its keys still run macros), clock (the Clock Extra's Earth time line). Needs ui.native.

- `piece` `string`
- `hidden?` `boolean`: default true; false shows it again

#### `ui.native.hidden(piece)`

Whether any mod hides a HUD piece.

- `piece` `string`

Returns `boolean`.

#### `ui.native.item_colour(item, colour)`

Colours an item's name in the game's shop and auction house lists while the mod is loaded ({r, g, b, a} 0..255; nil gives the game's colour back).

- `item` `integer`
- `colour` `table?`

#### `ui.native.config(id)`

A game config value by id (the Config window's settings).

- `id` `integer`

Returns `integer`.

#### `ui.native.set_config(id, value)`

Sets a game config value by id and applies it, as the Config window does (saved with the character's settings). Needs ui.native.

- `id` `integer`
- `value` `integer`

### `render`

How entities and the camera draw, and post-process passes.

#### `render.entity(id, look)`

How an entity draws while the mod is loaded: {hidden (model and plate), name (its plate's), name_colour {r, g, b, a}, alpha (0..1, at most its own), tint {r, g, b} multipliers, scale}. nil clears the mod's look for it; the newest mod's look wins.

- `id` `integer`: server id (ourselves too)
- `look` `table?`

#### `render.clear()`

Clears every look the mod set.

#### `render.shader(id, wgsl)`

A post-process pass over the world, drawn in the order set, under the screen fades and the 2D layer (nil removes it). The source is WGSL with `@fragment fn fs_main(in: VsOut) -> @location(0) vec4<f32>`; it has `scene` (the frame so far) and `samp`, `depth` (texture_depth_2d), `post.size`, `post.time` and `post.p[0..3]` (shader_params), and `in.uv`. A WGSL error is a log line, the pass then skipped.

- `id` `string`
- `wgsl` `string?`

#### `render.shader_params(id, params)`

A pass's four parameter vectors (post.p): up to four {x, y, z, w}.

- `id` `string`
- `params` `number[][]`

#### `render.camera_distance()`

The drawn eye's distance as a share of retail's (1 unless a mod sets it).

Returns `number`.

#### `render.set_camera_distance(scale)`

Sets the drawn eye's distance as a share of retail's (not in first person); as set_camera's distance.

- `scale` `number`: 0.5..3

#### `render.camera()`

The camera: {distance (the drawn eye's distance as a share of retail's, 1 unless a mod sets it), max_distance, speed (the pan speed's scale), battle_distance, first_person, eye, look (as drawn)}.

Returns `table`.

#### `render.set_camera(camera)`

Changes the camera (only the fields given): distance (the drawn eye's distance as a share of retail's, for this session; the camera keeps retail's distances for its swing, and the eye stops short of walls), max_distance (the most a distance may be, up to 10; 3 by default), speed (the pan speed's scale, 0.1..5), battle_distance (the distance while engaged, back to the player's after; false clears it).

- `camera` `{distance: number?, max_distance: number?, speed: number?, battle_distance: number|false|nil}`

### `audio`

The game's sounds and music, and the mod's own.

#### `audio.play(path, volume?)`

Plays a sound file from the mod's folder (16-bit PCM WAV, mono or stereo, any rate; or a game SeWave file) at the effects volume.

- `path` `string`
- `volume?` `number`: 0..1, default 1

#### `audio.play_se(id, volume?)`

Plays one of the game's sound effects by id (sound/win/se).

- `id` `integer`
- `volume?` `number`: 0..1

#### `audio.music(id?)`

Holds the music on a game track by id (the zone's choice of field, battle, night tracks stops until given back); nil gives it back, and the context's track plays again.

- `id?` `integer`

#### `audio.mute_music(on?)`

Silences the music while held (true, the default) or gives it back (false).

- `on?` `boolean`

#### `audio.play_ui(id)`

Plays an interface sound by id (the menus' cursor, select, cancel...).

- `id` `integer`

### `draw`

Lines, shapes, text and images placed in the world.

#### `draw.line(id, spec)`

Creates or replaces a line in the world: from, to ({x, y, z}, entity coordinates), colour {r, g, b, a}. It draws over the scene but behind what stands in front of it.

- `id` `string`
- `spec` `table`

#### `draw.ring(id, spec)`

Creates or replaces a horizontal ring: at, radius (yalms), colour, segments (default 48), filled.

- `id` `string`
- `spec` `table`

#### `draw.box(id, spec)`

Creates or replaces an axis-aligned wire box: at (its centre), size {x, y, z}, colour.

- `id` `string`
- `spec` `table`

#### `draw.text(id, spec)`

Creates or replaces a text at a world point: at {x, y, z}, text, size, colour, outline ({r, g, b, a}); centred there, drawn with the HUD and hidden behind what stands in front of it.

- `id` `string`
- `spec` `table`

#### `draw.image(id, spec)`

Creates or replaces a PNG from the mod's folder on a world quad: at {x, y, z} (its centre), w, h (world units), facing ('ground' (default): flat, the image's top toward yaw; 'upright': standing, its front toward yaw (read from there); 'camera': always turned to the camera), yaw (radians, 0 east as entity headings), colour (a tint); behind what stands in front of it.

- `id` `string`
- `spec` `table`

#### `draw.remove(id)`

Removes a shape.

- `id` `string`

#### `draw.clear()`

Removes every shape of the mod.

#### `draw.to_screen(point)`

Where a world point is on the screen this frame (window pixels, as hud places take), or nil behind the camera.

- `point` `{x: number, y: number, z: number}`

Returns `{x: integer, y: integer, on_screen: boolean}?`.

### `net`

HTTP, TCP, UDP, local pipes and sockets, and links (the net permission).

#### `net.request(spec, callback)`

Starts an HTTP(S) request on its own thread: {url, method (GET, POST, PUT, PATCH, DELETE, HEAD), body, headers, timeout (s)}; the callback later gets {status, body, headers} or {error}. Bodies past 16 MB are cut. Needs net.

- `spec` `table`
- `callback` `fun(res: table)`

Returns `integer`.

#### `net.connect(host, port, handlers)`

Opens a TCP connection (for a companion program); handlers {open(id), data(id, bytes), close(id, error?)} run in later mod phases. Needs net.

- `host` `string`
- `port` `integer`
- `handlers` `table`

Returns `integer`.

#### `net.pipe(name, handlers)`

Opens a local endpoint another program offers (Discord's IPC and the like): a Unix socket (a bare name is in the runtime folder, e.g. 'discord-ipc-0'; a path as given), on Windows the named pipe \\.\pipe\<name>; handlers and sending as net.connect's. Needs net.

- `name` `string`
- `handlers` `table`

Returns `integer`.

#### `net.listen(port, handlers, options?)`

Listens for TCP connections (on 127.0.0.1 unless options.host); port 0 picks one. handlers {accept(listener, id, peer_ip, peer_port), data(id, bytes), close(id, error?)} serve every connection. Returns the listener's id and its port. Needs net.

- `port` `integer`
- `handlers` `table`
- `options?` `{host: string?}`

Returns `integer, integer`.

#### `net.udp(port?, handlers, options?)`

Binds a UDP socket (127.0.0.1 unless options.host; nil or 0 picks a port); handlers {data(id, bytes, from_ip, from_port)}. Returns its id and port. Needs net.

- `port?` `integer`
- `handlers` `table`
- `options?` `{host: string?}`

Returns `integer, integer`.

#### `net.send(id, data, host?, port?)`

Sends bytes on a TCP connection, or a UDP datagram to host and port. Needs net.

- `id` `integer`
- `data` `string`
- `host?` `string`: UDP
- `port?` `integer`: UDP

#### `net.close(id)`

Closes a socket (a connection, a listener, a UDP socket).

- `id` `integer`

#### `net.open_url(url)`

Opens an http(s) URL in the browser. Needs net.

- `url` `string`

### `ipc`

Messages between game windows on this computer (multibox).

#### `ipc.send(text, character?)`

Sends text to the same mod in the other game windows on this machine (their ipc_message event), or only to one character's. Needs multibox.

- `text` `string`
- `character?` `string`

### `packets`

Hooks on the packets the server and the client send, and injecting them.

#### `packets.on_in(id, handler)`

Hooks received packets (one id, or every one when nil) before the client reads them. The handler gets {id, data, original, injected, blocked, outgoing} (data and original are the whole sub-packet, header included) and returns nil to leave it, false to block it, or new bytes. Hooks chain across mods: a blocked packet still reaches later ones. Needs packets.read; changing or blocking needs packets.write.

- `id` `integer?`: packet id
- `handler` `fun(p: vana.Packet): (string|false|nil)`

Returns `integer`.

#### `packets.on_out(id, handler)`

Hooks packets about to be sent, after the client built them; as on_in.

- `id` `integer?`: packet id
- `handler` `fun(p: vana.Packet): (string|false|nil)`

Returns `integer`.

#### `packets.off(id)`

Removes a packet hook.

- `id` `integer`: what on_in / on_out returned

#### `packets.inject_in(id_or_data, body?)`

Hands the client a packet as if the server had sent it (it passes the hooks, injected = true). Needs packets.write.

- `id_or_data` `integer|string`: an id, or a whole sub-packet
- `body?` `string`: the body after the 4-byte header

#### `packets.inject_out(id_or_data, body?)`

Sends a packet to the server (it passes the hooks, injected = true). Needs packets.write.

- `id_or_data` `integer|string`: an id, or a whole sub-packet
- `body?` `string`: the body after the 4-byte header

#### `packets.build(id, body)`

A whole sub-packet from an id and its body (the header's size made to fit).

- `id` `integer`
- `body` `string`

Returns `string`.

#### `packets.last(direction, id)`

The last packet of an id seen each way, or nil. Needs packets.read.

- `direction` `'in'|'out'`
- `id` `integer`

Returns `string?`.

### `chat`

The chat log and the chat line.

#### `chat.icon(path)`

A PNG of the mod's folder as an inline chat icon: a character to put in chat text (chat.add, a filters.chat rewrite), drawn where it stands at the line's height in both UIs' logs and the classic UI's game-font text.

- `path` `string`

Returns `string`.

#### `chat.add(text, how?)`

Adds a line to the chat log. With a mode it takes that mode's colour, filters and indent, like a game line; {colour = mode} takes only the mode's colour.

- `text` `string`
- `how?` `integer|{mode: integer?, colour: integer?}`: the game's chat mode (0..255), or a table

#### `chat.line()`

The chat line: whether it is open, and its text.

Returns `{open: boolean, text: string}`.

#### `chat.set_line(text?, open?)`

Opens the chat line with `text` (the player still presses Enter), or closes it with open = false.

- `text?` `string`
- `open?` `boolean`: default true

### `input`

The keys and the mouse: watching, taking and pressing them.

#### `input.on_key(handler)`

Sees every key press, repeat and release before the game and the UI do: {key, down, repeated, ctrl, alt, shift, typing, blocked}. Return true to take it from the game (needs input.capture; its repeats and release go with it). Filters chain across mods: a taken key still reaches later ones with blocked = true. It is the player's input: actions may act here.

- `handler` `fun(e: vana.KeyEvent): boolean?`

Returns `integer`.

#### `input.on_mouse(handler)`

Sees the mouse before the game and the UI do: {kind = 'down' | 'up' | 'move' | 'wheel', button (left, right, middle, back, forward), x, y (window pixels), delta (wheel notches, up positive), blocked}. Return true to take it (needs input.capture).

- `handler` `fun(e: vana.MouseEvent): boolean?`

Returns `integer`.

#### `input.off(id)`

Removes a key or mouse filter.

- `id` `integer`: what on_key / on_mouse returned

#### `input.held(key)`

Whether a key is held (a name as on_key gives it, or shift / ctrl / alt / win for either side).

- `key` `string`

Returns `boolean`.

#### `input.modifiers()`

The modifier keys held.

Returns `{ctrl: boolean, alt: boolean, shift: boolean, win: boolean}`.

#### `input.mouse()`

The pointer (window pixels) and its buttons.

Returns `{x: number, y: number, left: boolean, right: boolean, middle: boolean}`.

#### `input.typing()`

Whether a text field (the chat line, a search, the map's...) has the keys.

Returns `boolean`.

#### `input.press(key)`

Presses and releases a key, with modifiers ("enter", "ctrl+f1", "^1"), as the keyboard would this frame. Needs input.capture.

- `key` `string`

#### `input.send(key, down)`

Holds a key down or lets it go. Needs input.capture.

- `key` `string`
- `down` `boolean`

### `binds`

Key binds that run lines, as the game's own binds do.

#### `binds.set(key, action, options?)`

Binds a key with exact modifiers ("f9", "ctrl+1", Windower's "^!f1": ^ Ctrl, ! Alt, ~ Shift, @ Win) to a line run as typed or a function. The key is taken from the game. A line other than a //mod command is a game action (needs actions). Options: up (fire on release), typing (fire while a text field is open too).

- `key` `string`
- `action` `string|function`
- `options?` `{up: boolean?, typing: boolean?}`

#### `binds.unset(key)`

Removes the mod's binds of a key.

- `key` `string`

#### `binds.list()`

The mod's binds.

Returns `{key: string, line: string?, up: boolean, typing: boolean}[]`.

### `timer`

Calls after a delay or at an interval.

#### `timer.after(seconds, handler)`

Calls `handler` once after `seconds`.

- `seconds` `number`
- `handler` `function`

Returns `integer`.

#### `timer.every(seconds, handler)`

Calls `handler` every `seconds` until cancelled.

- `seconds` `number`: at least 0.01
- `handler` `function`

Returns `integer`.

#### `timer.cancel(id)`

Stops a timer.

- `id` `integer`: what timer.after / timer.every returned

### `game`

The game's state: our character, the zone, entities, party, items, jobs and more.

#### `game.player()`

Our character: name, id, index, zone, pos {x, y, z}, heading, status, hp / mp / tp and their percents, maxima, jobs and levels, target id.

Returns `vana.Player`.

#### `game.zone()`

The zone id, or nil while zoning.

Returns `integer?`.

#### `game.entity(key)`

An entity in range by server id or index, or nil.

- `key` `integer`: server id or index

Returns `vana.Entity?`.

#### `game.entities(filter?)`

Every entity in range, by index.

- `filter?` `{distance: number?, kinds: ('player'|'npc'|'mob')[]?}`

Returns `vana.Entity[]`.

#### `game.target()`

The current target, or nil.

Returns `vana.Entity?`.

#### `game.party()`

The party and alliance members (slot 0..5 our party, 6..17 the others).

Returns `vana.Member[]`.

#### `game.items(bag)`

A bag's items in the list's order (0 inventory, 1 safe, 2 storage, 3 temporary, 4 locker, 5 satchel, 6 sack, 7 case, 8.. wardrobes).

- `bag` `integer`

Returns `vana.Item[]`.

#### `game.bag_size(bag)`

How many slots a bag has (nil before the server said).

- `bag` `integer`

Returns `integer?`.

#### `game.equipment()`

What is equipped, by equipment slot (0 main .. 15 back): {bag, slot, id}.

Returns `table<integer, {bag: integer, slot: integer, id: integer}>`.

#### `game.buffs()`

Our status effects: id, and seconds remaining when timed.

Returns `{id: integer, remaining: number?}[]`.

#### `game.recasts()`

Seconds left on recasts: spells by spell id, abilities by recast id (only those still counting).

Returns `{spells: table<integer, number>, abilities: table<integer, number>}`.

#### `game.time()`

The Vana'diel clock: earth (seconds since the Vana'diel epoch), year, month, day, hour, minute, weekday (0 Firesday), moon_phase, moon (percent).

Returns `vana.Time`.

#### `game.weather()`

The weather id.

Returns `integer`.

#### `game.key_items()`

The key item ids we hold.

Returns `integer[]`.

#### `game.treasure()`

The treasure pool: {slot (0..9), item, count, dropper, lotted, passed, lot, high_lot, high_name} per item in it.

Returns `table[]`.

#### `game.event()`

The event (cutscene, NPC talk) under way, or nil: {menu = {question, options, cursor}} while it shows a menu (cursor from 1).

Returns `table?`.

#### `game.entity_by_tag(tag)`

The entity a target tag names (t, st, bt, pet, me, ht, scan, p0..p5, a10..a25, with or without <>), a player's name or a server id; nil when none.

- `tag` `string`

Returns `vana.Entity?`.

#### `game.skills()`

Our skills by id: combat (1..31) and magic (32..47) {level, capped}; crafts (48..) {rank, level, capped}.

Returns `{combat: table<integer, table>, magic: table<integer, table>, crafts: table<integer, table>}`.

#### `game.jobs()`

Each unlocked job's level, and the master levels, by job id.

Returns `{levels: table<integer, integer>, master: table<integer, integer>}`.

#### `game.stats()`

Base and bonus stats (STR..CHR), attack, defense, resistances, item and superior level, title, rank and points, nation, home point, experience and the next level's, master level and exemplar points, limit and merit points; nil before the server sent them.

Returns `table?`.

#### `game.merits()`

Each merit we have by index: {level, next_cost}.

Returns `table<integer, {level: integer, next_cost: integer}>`.

#### `game.job_points()`

Capacity points, job points and job points spent, by job id.

Returns `table<integer, {capacity: integer, points: integer, spent: integer}>`.

#### `game.gil()`

Our gil.

Returns `integer`.

#### `game.spells()`

The spell ids we have learned.

Returns `integer[]`.

#### `game.abilities()`

The ability ids (res.ability) we can use now: job abilities, weapon skills, pet commands with a pet.

Returns `integer[]`.

#### `game.job_data()`

The job setups of main and sub: job, raw (the server's bytes), and for BLU blue_spells (ids), for PUP head, frame and attachments (item ids).

Returns `{main: table?, sub: table?}`.

#### `game.macros()`

The macro palette: {book, set (1-based), book_title, books (the 40 titles), ctrl, alt (10 macros each: {title, lines (6)})}; nil before the macros load.

Returns `table?`.

#### `game.map()`

The map the client shows where we stand (a map it has and we own): {zone, number (the map's page), file (its DAT id), scale, origin {x, y} (the image's top left in map pixels), size (512), px_per_unit (map pixels per world unit)}; nil with none.

Returns `table?`.

#### `game.map_pos(x, z)`

A world position (x, z) as a pixel on the current map's 512 x 512 image; nil without a map.

- `x` `number`
- `z` `number`

Returns `integer?, integer?`.

#### `game.collision(x, z, radius?)`

The zone's collision triangles with a corner within `radius` (8 by default, 64 at most) of (x, z): each {1, 2, 3 (corners {x, y, z}), normal, kind (the surface type)}, in world coordinates (y down, as positions); open doors left out. nil before the zone's collision loads.

- `x` `number`
- `z` `number`
- `radius?` `number`

Returns `table[]?`.

#### `game.collision_mesh()`

Every collision triangle of the zone as packed little-endian floats, 36 bytes a triangle (three corners x, y, z): for export and navmesh building. nil before it loads.

Returns `string?`.

#### `game.raycast(from, to)`

The first collision triangle on the segment from -> to: the hit {x, y, z, distance}, or nil.

- `from` `{x: number, y: number, z: number}`
- `to` `{x: number, y: number, z: number}`

Returns `table?`.

#### `game.floor(x, z, y?)`

The height a character at (x, y, z) stands on (the first floor below, else the nearest surface); y defaults to ours.

- `x` `number`
- `z` `number`
- `y?` `number`

Returns `number?`.

#### `game.cast()`

Our cast bar while it shows: {spell (none for an event's bar), total and left (seconds), progress 0..1, event}.

Returns `table?`.

#### `game.info()`

The session: zone, server, mog_house, chat_open, menu_open, event, in_combat, locked (target lock), autorun, follow (server id), pet_index, map_grid (as <pos>), modern_ui.

Returns `table`.

#### `game.camera()`

The camera: eye and look points, and their distance.

Returns `{eye: {x: number, y: number, z: number}, look: {x: number, y: number, z: number}, distance: number}`.

#### `game.shop()`

The open NPC shop's rows ({index from 1, item, price}), or nil (no shop, or a guild's).

Returns `table[]?`.

#### `game.npc_names()`

The zone's NPC list by server id, spawned or not (Wide Scan's names).

Returns `table<integer, string>`.

### `actions`

Doing things in the game: casting, items, equipment, targeting, moving, menus (actions or automation).

#### `actions.cast(spell, target)`

Casts a spell by id, with the magic menu's checks (recast, MP, learned, status) and its refusal lines.

- `spell` `integer`: spell id
- `target` `integer|string?`: a server id, a tag (<t>, <me>, <bt>, <st>...) or a name; none: the current target

#### `actions.ability(id, target)`

Uses a job ability, weapon skill or pet command by record id (res.ability), with the ability menu's checks.

- `id` `integer`: ability id
- `target` `integer|string?`: as actions.cast

#### `actions.weaponskill(id, target)`

As actions.ability (weapon skills are ability records).

- `id` `integer`: ability id
- `target` `integer|string?`: as actions.cast

#### `actions.ranged(target)`

A ranged attack (/range).

- `target` `integer|string?`: as actions.cast

#### `actions.use_item(item, target)`

Uses an item by id, as /item finds it (temporary items, inventory, wardrobes).

- `item` `integer`: item id
- `target` `integer|string?`: as actions.cast

#### `actions.use_slot(bag, index, target)`

Uses the item in a bag slot, as the item menu does.

- `bag` `integer`
- `index` `integer`
- `target` `integer?`: a server id; none: ourselves

#### `actions.macro(row, n)`

Runs a macro of the current set, as its key does: row 'ctrl' or 'alt', 1..10.

- `row` `string`
- `n` `integer`

#### `actions.macro_set(book, set)`

Switches the macro palette to a book (1..40) and set (1..10).

- `book` `integer`
- `set` `integer`

#### `actions.macro_edit(row, n, line, text)`

Changes a macro of the current set (and saves the set): its line 1..6, or 'title'.

- `row` `string`
- `n` `integer`
- `line` `integer|'title'`
- `text` `string`

#### `actions.attack(target)`

Engages a target, or switches to it while engaged (/attack).

- `target` `integer|string?`: none: the current target

#### `actions.disengage()`

Disengages (/attack off).

#### `actions.assist(target)`

Assists a player (/assist).

- `target` `integer|string`

#### `actions.target(id)`

Sets the current target, or clears it with nil.

- `id` `integer?`: server id

#### `actions.lock(on)`

Locks on to the current target, or lets go.

- `on` `boolean`

#### `actions.heal(on)`

Rests (/heal): true on, false off, none toggles.

- `on` `boolean?`

#### `actions.equip(slot_or_set, item)`

Wears pieces as the equipment window does: (slot, item) or ({slot = item, ...}). A slot is 0..15 or a name (main, sub, range, ammo, head, body, hands, legs, feet, neck, waist, left_ear, right_ear, left_ring, right_ring, back); an item is an id, {bag = , index = }, or false to take it off. Pieces already worn stay.

- `slot_or_set` `integer|string|table`
- `item` `integer|table|false|nil`

#### `actions.unequip(slot)`

Takes off what a slot wears.

- `slot` `integer|string`

#### `actions.equip_set(n)`

Wears an equip set (/equipset).

- `n` `integer`: 1..200

#### `actions.move(bag, index, to, count)`

Moves an item to another bag (its first free slot).

- `bag` `integer`
- `index` `integer`
- `to` `integer`: bag
- `count` `integer?`: none: the whole stack

#### `actions.sort(bag)`

Sorts a bag and merges its stacks.

- `bag` `integer`

#### `actions.drop(index)`

Drops an inventory slot's whole stack (no confirmation).

- `index` `integer`: inventory slot

#### `actions.lot(slot)`

Lots on a treasure pool item.

- `slot` `integer`: 0..9, as game.treasure gives it

#### `actions.pass(slot)`

Passes on a treasure pool item.

- `slot` `integer`: 0..9

#### `actions.cancel(status)`

Cancels a status effect we have, when it can be cancelled.

- `status` `integer`: status id

#### `actions.choose(option)`

Picks an option of the event menu showing (game.event().menu), as the cursor and Enter would.

- `option` `integer`: from 1

#### `actions.advance()`

Moves the event's waiting text on, as Enter does (enternity, stepdialog).

#### `actions.open_menu(name)`

Opens a game window as its main menu row does (refused as there when the row is off): status, equipment, magic, items, abilities, party, trade, search, linkshell, synthesis, region, map (the moogle in a Mog House), missions, quests, key_items, view_house, bazaar, macros, config, help_desk, communication, clock, shut_down, log_out; or moogle (the nomad moogle menu, as a nomad moogle opens it).

- `name` `string`

#### `actions.trade(npc, items?, gil?)`

Hands items (and gil) to an NPC within reach, as the trade window's Trade does: items {{slot = inventory slot, count = n}, ...}, at most 8 entries with the gil. Not worn or linkshell items. A refusal is an error.

- `npc` `integer`: server id
- `items?` `table`
- `gil?` `integer`

#### `actions.buy(row, count?)`

Buys from the open NPC shop (game.shop()'s rows), as its Buy does.

- `row` `integer`: from 1
- `count?` `integer`: default 1

#### `actions.sell(slot, count?)`

Sells from an inventory slot to the open NPC shop, as its Sell does.

- `slot` `integer`: inventory slot
- `count?` `integer`: default 1

#### `actions.blue_set(slot, spell)`

Sets a blue magic slot (1..20) to a spell id, as Set Spells does.

- `slot` `integer`
- `spell` `integer`

#### `actions.blue_remove(slot)`

Empties a blue magic slot (1..20).

- `slot` `integer`

#### `actions.attach(slot, item)`

Sets an automaton slot to an item id: 0 head, 1 frame, 2..13 attachments.

- `slot` `integer`
- `item` `integer`

#### `actions.detach(slot)`

Empties an automaton slot (0..13).

- `slot` `integer`

#### `actions.invite(name)`

Invites a player to the party (/pcmd add).

- `name` `string`

#### `actions.party_leave()`

Leaves the party (/pcmd leave).

#### `actions.party_kick(name)`

Removes a member from the party (/pcmd kick).

- `name` `string`

#### `actions.join()`

Accepts a party invitation (/join).

#### `actions.decline()`

Declines a party invitation (/decline).

#### `actions.mount(name)`

Calls a mount by name (/mount).

- `name` `string`

#### `actions.dismount()`

Dismounts (/dismount).

#### `actions.talk(id)`

Talks to an NPC (or checks it), as Enter on it does.

- `id` `integer`: server id

#### `actions.scan()`

Asks for the Wide Scan list.

#### `actions.track(index)`

Tracks an entity with Wide Scan.

- `index` `integer`: its index (entity.index)

#### `actions.untrack()`

Stops Wide Scan tracking.

#### `actions.run(heading)`

Runs on (autorun) toward a heading (entity.heading's units), or the way we face. The player's keys still steer and stop it.

- `heading` `number?`

#### `actions.run_to(x, z, within)`

Runs to a point and stops within a distance of it.

- `x` `number`
- `z` `number`
- `within` `number?`: yalms; 1 when none

#### `actions.follow(id)`

Follows an entity (/follow).

- `id` `integer`: server id

#### `actions.walk(on)`

Walks (true) or runs (false), as the walk key.

- `on` `boolean`

#### `actions.stop()`

Stops a run, run_to or follow.

#### `actions.turn(heading)`

Faces a heading (entity.heading's units).

- `heading` `number`

### `res`

Game data from the DATs: items, spells, abilities, statuses, zones, messages, names.

#### `res.item(id)`

An item's record: name, description, kind, flags, stack, type, targets, level, slots, races, jobs (bits), superior_level, damage, delay, skill, max_charges, cast_time, use_delay, reuse_delay.

- `id` `integer`

Returns `table?`.

#### `res.spell(id)`

A spell's record: name, description, kind (1 white .. 8 trust), element, targets (bits), skill, mp, cast_time and recast (seconds), range, levels (by job).

- `id` `integer`

Returns `table?`.

#### `res.ability(id)`

An ability's record by its name-table id (weapon skills 0..0x1ff, job abilities 0x200.., pet commands 0x400..): name, description, kind, cost, recast_id, targets, tp, level.

- `id` `integer`

Returns `table?`.

#### `res.status(id)`

A status effect's name.

- `id` `integer`

Returns `string?`.

#### `res.zone(id)`

A zone's name.

- `id` `integer`

Returns `string?`.

#### `res.title(id)`

A title's name.

- `id` `integer`

Returns `string?`.

#### `res.key_item(id)`

A key item's name and description.

- `id` `integer`

Returns `{id: integer, name: string, description: string?}?`.

#### `res.augments(item, extdata)`

An item's augments from its extdata (game.items' extdata): lines (the item window's text), short (its <...> line: an evolith, a serial, a rank) and pairs {id, value} (the message id and value of each); empty for an item without them. Mod phase only.

- `item` `integer`: item id
- `extdata` `string`: 24 bytes

Returns `{lines: string[]?, short: string?, pairs: {id: integer, value: integer}[]?}`.

#### `res.message(id, options?)`

A battle / system message expanded as the log prints it: options {actor, target (server ids or names), values {param1, param2, ...}}; {text, mode} or nil for an id without text.

- `id` `integer`: message id
- `options?` `table`

Returns `{text: string, mode: integer?}?`.

#### `res.autotranslate(word?, group?)`

The auto-translate dictionary: the entries whose completion key starts with `word` (none: the group titles), or a group's entries: {text, group, id, phrase (the in-line string: put it in a chat line)}; nil before the dictionary loads.

- `word?` `string`
- `group?` `integer`

Returns `table[]?`.

#### `res.autotranslate_groups()`

The auto-translate groups: {group, title, count}.

Returns `table[]?`.

#### `res.name(kind, id, lang?)`

A name in a language's tables: kind item, key_item, spell, ability (name-table id), zone or title; lang ja, en (the default), de or fr. Another language than the client's loads on its first call: nil until its tables are in (a moment).

- `kind` `string`
- `id` `integer`
- `lang?` `string`

Returns `string?`.

#### `res.dat(id)`

A game file's bytes by file id, as the client reads it (through the mods' DAT overlay); nil when the install has none.

- `id` `integer`: file id

Returns `string?`.

#### `res.dat_path(id)`

Where a file id is read from (a mod's file when an overlay replaces it).

- `id` `integer`: file id

Returns `string?`.

#### `res.model_files(model)`

The file ids a model loads: an NPC model id gives {file}; a look {race, face, head, body, hands, legs, feet, main, sub, range} (Entity.look with its race) gives {skeleton, parts = {slot, model, file}}.

- `model` `integer|table`

Returns `table`.

#### `res.weather(id)`

A weather's name by id (0 Clear .. 19 Darkness).

- `id` `integer`

Returns `string?`.

#### `res.weather_element(id)`

The element a weather brings (0 Fire, 1 Ice, 2 Wind, 3 Earth, 4 Lightning, 5 Water, 6 Light, 7 Dark), nil for none.

- `id` `integer`

Returns `integer?`.

#### `res.element(id)`

An element's name (0 Fire .. 7 Dark).

- `id` `integer`

Returns `string?`.

#### `res.mount(id)`

A mount's name by its list index.

- `id` `integer`

Returns `string?`.

#### `res.mob_skill(id)`

A monster ability's name (ids from 256).

- `id` `integer`

Returns `string?`.

#### `res.skill(id)`

A combat or magic skill's name by skill id.

- `id` `integer`

Returns `string?`.

#### `res.day(weekday)`

A Vana'diel weekday's name (0 Firesday).

- `weekday` `integer`

Returns `string?`.

#### `res.moon(phase)`

A moon phase's name.

- `phase` `integer`

Returns `string?`.

#### `res.job(id)`

A job's abbreviation (1 WAR .. 22 RUN).

- `id` `integer`

Returns `string?`.

### `fs`

Files in the mod's data folder (and its own files, read only).

#### `fs.read(path)`

A file's bytes, or nil: the mod's data folder first (what it wrote), then its own files. "shared/..." is the area every mod with files.shared can reach.

- `path` `string`: relative, / separated

Returns `string?`.

#### `fs.write(path, data)`

Writes a file in the mod's data folder (folders made as needed).

- `path` `string`
- `data` `string`

#### `fs.append(path, data)`

Adds to the end of a file in the mod's data folder.

- `path` `string`
- `data` `string`

#### `fs.exists(path)`

Whether a file or folder is there (data folder or the mod's own files).

- `path` `string`

Returns `boolean`.

#### `fs.list(path?)`

The names in a folder, sorted (folders end in /): the data folder's and the mod's own.

- `path?` `string`: none: the top

Returns `string[]`.

#### `fs.mkdir(path)`

Makes a folder (and its parents) in the data folder.

- `path` `string`

#### `fs.remove(path)`

Removes a file or a folder with everything in it, from the data folder.

- `path` `string`

#### `fs.rename(from, to)`

Moves a file or folder within the data folder.

- `from` `string`
- `to` `string`

### `json`

JSON text to Lua tables and back.

#### `json.encode(value, options?)`

Plain data as JSON text.

- `value` `any`
- `options?` `{pretty: boolean?}`

Returns `string`.

#### `json.decode(text)`

JSON text as Lua values.

- `text` `string`

Returns `any`.

### `mods`

The other mods: the list, calls into them, events to them.

#### `mods.list()`

Every loaded mod: id, name, version, the functions it provides, its time a frame (avg_ms, peak_ms), memory_kb and recent errors.

Returns `{id: string, name: string, version: string, provides: string[], avg_ms: number, peak_ms: number, memory_kb: integer?, errors: integer}[]`.

#### `mods.installed()`

Every mod on disk and bundled: {id, name, version, enabled, bundled, loaded}. Load or unload one with commands.run('/lua load <id>') / '/lua unload <id>'.

Returns `table[]`.

#### `mods.provide(name, func)`

Offers a function to other mods (mods.call). Values cross as copies of plain data.

- `name` `string`
- `func` `function`

#### `mods.call(mod, name, ...)`

Calls another mod's provided function now, in its VM, and returns what it returned (copies). It cannot sleep.

- `mod` `string`: its id
- `name` `string`
- `...` `any`

Returns `any`.

#### `mods.emit(event, ...)`

Raises an event in every other mod: their events.on handlers get the values once the running handler is done (the same mod phase). Prefix the name with your mod's id.

- `event` `string`
- `...` `any`: plain data

### `text`

Shift-JIS and auto-translate phrases in strings.

#### `text.from_shift_jis(bytes)`

Shift-JIS bytes (packets, game files) as text.

- `bytes` `string`

Returns `string`.

#### `text.to_shift_jis(text)`

Text as Shift-JIS bytes.

- `text` `string`

Returns `string`.

#### `text.phrase(group, id)`

An auto-translate phrase as a line holds it (group, id from res.autotranslate); put it in chat text.

- `group` `integer`
- `id` `integer`

Returns `string`.

#### `text.autotranslate(text)`

The phrase whose text is exactly `text` (ignoring case), as a line holds it; nil when the dictionary has none.

- `text` `string`

Returns `string?`.

#### `text.plain(text)`

A line with its auto-translate phrases as plain words.

- `text` `string`

Returns `string`.

### `client`

The client itself: the window, the clipboard, screenshots, recording, programs, the process, switches.

#### `client.copy(text)`

Puts text on the clipboard.

- `text` `string`

#### `client.paste(handler)`

Reads the clipboard's text after this frame and hands it to the handler. Needs clipboard.

- `handler` `fun(text: string)`

#### `client.record(name?, options?)`

Records the game window as drawn to an H.264 video (ffmpeg must be installed) under the client's ffxiuser/videos folder, as `name`.mp4 (the date and time when none): options {fps (30)}. It ends with stop_recording, a new window size, or the mod unloading. Returns the file's path. Needs exec.

- `name?` `string`
- `options?` `{fps: integer?}`

Returns `string`.

#### `client.stop_recording()`

Ends the recording (ffmpeg finishes the file).

#### `client.recording()`

The file being recorded, or nil (as of the last frame).

Returns `string?`.

#### `client.process()`

The client's process: pid, cores (how many), affinity (the cores it may run on, from 0; empty where unknown) and priority (idle, low, below_normal, normal, above_normal, high).

Returns `{pid: integer, cores: integer, affinity: integer[], priority: string}`.

#### `client.set_affinity(cores)`

Runs every thread of the client on these cores (from 0). Needs process. Returns true, or false and why.

- `cores` `integer[]`

Returns `boolean, string?`.

#### `client.set_priority(level)`

The client's scheduling priority: idle, low, below_normal, normal, above_normal or high (above normal usually needs privileges on Linux). Needs process. Returns true, or false and why.

- `level` `string`

Returns `boolean, string?`.

#### `client.option(name)`

A behaviour switch's value (see set_option); a value set shows from the next frame.

- `name` `string`

Returns `boolean|number|table`.

#### `client.set_option(name, value)`

Sets a behaviour switch while the mod is loaded (retail's back when it unloads): macro_palette_delay (false: the Ctrl / Alt palette opens at once, not at the next whole second), ah_result_delay (false: a bid's result prints at once), equipset_delay (false: no 0.9 s wait between equip sets), log_slide (false: new log lines show at once), cutscene_speed (1..4: events play that much faster), animation_rate (0.25..4: the actors' animation speed), clear_colour and ambient ({r, g, b} 0..255 in place of the zone's background colour and ambient light; false gives the zone's back), minimap_span (the minimap's zoom, map pixels across, 48..512; kept as the minimap's own wheel keeps it); rules: move_lock (false: our abilities' and the weapon draw's move lock is lifted), all_maps (true: every map without its key item), dead_commands (true: text commands while K.O.), compass_monsters (true: monster dots on the compass for every job), player_collision (false: moving players never hold a step), occlusion (false: characters behind scenery still draw and target), menu_reach (false: the auction house and the nomad moogle menu stay open as we move away); send_interval (ms between the datagrams we send, 50..1000; retail 400); wireframe (true: the scenery drawn as lines, where the GPU draws them); ttf_text (true: the game font's text in the UI's TrueType face in the classic UI too: the log, the windows' text; sprite numbers and captions stay retail's).

- `name` `string`
- `value` `boolean|number|table`

#### `client.window()`

The game window: {x, y (its place on the desktop, when the system tells it), w, h (pixels), focused}.

Returns `{x: integer?, y: integer?, w: integer, h: integer, focused: boolean}`.

#### `client.set_window(window)`

Changes the game window: moves ({x, y}), sizes ({w, h}), top (always on top), minimised, fullscreen (borderless, on its monitor), border (the system frame), capture = false (kept out of screen captures where the system allows it). Only the fields given change. Needs multibox.

- `window` `{x: integer?, y: integer?, w: integer?, h: integer?, top: boolean?, minimised: boolean?, fullscreen: boolean?, border: boolean?, capture: boolean?}`

#### `client.focus()`

Asks for the keyboard focus for this game window (switching between clients). Needs multibox.

#### `client.screenshot(name?)`

Saves a screenshot of the next frame as drawn, under the game's screenshots folder (subfolders allowed; .png added); returns the name used.

- `name?` `string`

Returns `string`.

#### `client.run(program, args?, handler?)`

Starts a program with its arguments (no shell). With a handler, it gets {status (exit code), body (stdout), headers = {stderr}} when the program ends, or {error}. Returns the process id. Needs exec.

- `program` `string`
- `args?` `string[]`
- `handler?` `fun(result: table)`

Returns `integer`.

### `devices`

USB HID devices by their raw reports (the devices permission).

#### `devices.list()`

The HID devices: {path, vendor, product, name, interface (the USB interface, when known), descriptor (the report descriptor's bytes, Linux)}. On Linux the hidraw nodes (opening one needs read and write access to it); on Windows the HID interfaces (the system holds keyboards and mice itself). Needs devices.

Returns `table[]`.

#### `devices.open(path, handlers)`

Opens a device by its path; handlers {open(id), data(id, report), close(id, error?)} run in later mod phases, one data call a report (the report id first where the device numbers them). Returns its id. Needs devices.

- `path` `string`
- `handlers` `table`

Returns `integer`.

#### `devices.write(id, report)`

Sends an output report (its report id first, 0 when the device has none; on Windows exactly the report's length). Needs devices.

- `id` `integer`
- `report` `string`

#### `devices.close(id)`

Closes a device.

- `id` `integer`

### `storage`

The mod's saved data (storage.account, storage.character) and sealed secrets.

#### `storage.save()`

Writes storage.account and storage.character now (they are also written at logout and unload).

#### `storage.seal(text)`

Seals text (a password) with this machine's key, for storage: only this mod on this machine can open it.

- `text` `string`

Returns `string`.

#### `storage.unseal(sealed)`

Opens text storage.seal made; nil when it does not open (another machine or mod, or damaged).

- `sealed` `string`

Returns `string?`.

### `lobby`

The screens before login, for lobby mods.

#### `lobby.state()`

The lobby before login (a mod with `lobby = true` in mod.toml runs there): screen (login, title, select, create, delete, config, connecting, world), server, account, playonline, busy (a fade, a reply awaited or an error up: actions wait), error, selected (a slot) and characters {slot, name, world, race, job, level, sub_job, zone, available}.

Returns `table`.

#### `lobby.sign_in(login)`

Signs in from the login screen as its Log in button does: {account, password, otp?, server?}, and for a PlayOnline server pol_id and pol_password. Needs automation (or actions in the player's input).

- `login` `table`

#### `lobby.play(character)`

Logs in with a character from the title or select screen, as the select screen's Play does: its name or its slot. Needs automation (or actions in the player's input).

- `character` `string|integer`

### `settings`

The settings declared in mod.toml.

#### `settings.get(key)`

A setting declared in mod.toml: the user's value, else its default.

- `key` `string`

Returns `any`.

### Event list

Each is watched with `events.on(name, handler)`; the handler gets the arguments listed.

| Event | Arguments | When |
| --- | --- | --- |
| `load` | - | The mod's entry file has run. |
| `unload` | - | The mod is about to be unloaded (logout, /lua unload or reload; a lobby mod as the client closes). |
| `login` | `name: string` | The character is in the world, once per session. |
| `lobby_screen` | `screen: string`, `previous: string?` | The lobby's screen changed (see lobby.state; world once in the world). Lobby mods get it from the start. |
| `logout` | - | The character is leaving the world. |
| `zone_change` | `zone: integer`, `previous: integer?` | A new zone has loaded. |
| `settings_changed` | `key: string`, `value: any` | The user changed one of the mod's settings (its Extras page, or /lua set). |
| `focus_change` | `focused: boolean` | The game window gained or lost the keyboard focus. |
| `emote` | `actor: integer`, `target: integer`, `emote: integer`, `mode: integer` | Someone used an emote (s2c 0x05A): who, at whom (0 none), the emote id, its mode (motion only, text only...). |
| `party_invite` | `from: string`, `kind: 'party'\|'alliance'` | We were invited to a party or an alliance (s2c 0x0DC). |
| `examined` | `id: integer`, `name: string` | Someone examined us (/check; message 89). |
| `frame` | `dt: number` | Every frame (prefer timers and events: this one costs time every frame). |
| `hp_change` | `hp: integer`, `previous: integer` | Our HP changed. |
| `mp_change` | `mp: integer`, `previous: integer` | Our MP changed. |
| `tp_change` | `tp: integer`, `previous: integer` | Our TP changed. |
| `hpp_change` | `hpp: integer`, `previous: integer` | Our HP percent changed. |
| `mpp_change` | `mpp: integer`, `previous: integer` | Our MP percent changed. |
| `status_change` | `status: integer`, `previous: integer` | Our status changed (0 idle, 1 engaged, 2 / 3 dead, 33 resting...). |
| `job_change` | `main: integer`, `main_level: integer`, `sub: integer`, `sub_level: integer` | Our main or sub job changed. |
| `level_up` | `level: integer` | Our main job's level went up. |
| `level_down` | `level: integer` | Our main job's level went down. |
| `buff_gain` | `id: integer` | We gained a status effect. |
| `buff_lose` | `id: integer` | We lost a status effect. |
| `target_change` | `id: integer?`, `previous: integer?` | Our target changed (nil: none). |
| `time_change` | `minute: integer`, `previous: integer` | A Vana'diel minute passed (minutes since midnight). |
| `day_change` | `weekday: integer`, `previous: integer` | A new Vana'diel day (0 Firesday). |
| `moon_change` | `phase: integer`, `previous: integer` | The moon's phase changed. |
| `weather_change` | `weather: integer`, `previous: integer` | The weather changed. |
| `party_change` | - | The party or alliance changed (members joined, left or moved). |
| `item_add` | `bag: integer`, `slot: integer`, `id: integer`, `count: integer` | An item appeared in a bag slot. |
| `item_remove` | `bag: integer`, `slot: integer`, `id: integer`, `count: integer` | An item left a bag slot. |
| `chat_message` | `text: string`, `sender: string`, `kind: integer`, `gm: boolean` | A chat line from a player (or a GM): its text, sender, kind (0 say, 1 shout, 3 tell, 4 party, 5 linkshell, 0x1a yell, 0x1b linkshell 2, 0x21 unity) and whether a GM sent it. |
| `action` | `action: vana.Action` | A decoded action packet (s2c 0x028): actor, category, param, info and targets with their results. |
| `battle_message` | `actor: integer`, `target: integer`, `message: integer`, `param1: integer`, `param2: integer` | A battle message (s2c 0x029 / 0x02D): actor, target, message id and its two parameters. |
| `damage` | `actor: integer`, `target: integer`, `value: integer`, `message: integer` | A damage number: a battle message that puts one up (main and proc records), whatever the chat filters show. |
| `ipc_message` | `text: string`, `from: string` | Text the same mod sent from another game window on this machine (ipc.send; needs multibox). |
| `item_change` | `bag: integer`, `slot: integer`, `id: integer`, `count: integer`, `previous: integer` | A bag slot's count changed. |

### Types

Tables the API hands out or takes. A type ending in `?` may be nil.

#### `vana.Player`

Our character (game.player): its ids, zone, place, vitals, jobs and target.

| Field | Type | Note |
| --- | --- | --- |
| `name` | `string` |  |
| `id` | `integer` |  |
| `index` | `integer` |  |
| `zone` | `integer?` |  |
| `pos` | `{x: number, y: number, z: number}` | where we stand: x and z on the ground, y down (a smaller y is higher) |
| `heading` | `number` | radians, 0 to 2π |
| `status` | `integer` | the server's status: 0 idle, 1 engaged, 2 and 3 K.O., 5 mounted, 33 resting |
| `hp` | `integer?` |  |
| `mp` | `integer?` |  |
| `tp` | `integer?` |  |
| `hpp` | `integer?` |  |
| `mpp` | `integer?` |  |
| `hp_max` | `integer?` |  |
| `mp_max` | `integer?` |  |
| `main_job` | `integer?` |  |
| `main_level` | `integer?` |  |
| `sub_job` | `integer?` |  |
| `sub_level` | `integer?` |  |
| `target` | `integer?` | our target's server id |

#### `vana.Entity`

A character, NPC or monster in the zone (game.entity, game.entities, game.target); ours too, with `me`.

| Field | Type | Note |
| --- | --- | --- |
| `id` | `integer` |  |
| `index` | `integer` |  |
| `name` | `string` |  |
| `kind` | `'player'\|'npc'\|'mob'` |  |
| `pos` | `{x: number, y: number, z: number}` | x and z on the ground, y down |
| `heading` | `number` | radians, 0 to 2π |
| `distance` | `number` | from us, in yalms |
| `hpp` | `integer` |  |
| `status` | `integer` |  |
| `speed` | `number` |  |
| `claim` | `integer` |  |
| `targetable` | `boolean` |  |
| `hidden` | `boolean` |  |
| `master` | `integer` | the master's index (pets), 0 for none |
| `me` | `boolean?` | true for our own character |
| `race` | `integer?` |  |
| `model` | `integer` | a monster or NPC model id (0 for a player-style look) |
| `look` | `{face: integer, head: integer, body: integer, hands: integer, legs: integer, feet: integer, main: integer, sub: integer, range: integer}?` | a player-style look |
| `in_party` | `boolean` |  |
| `in_alliance` | `boolean` |  |
| `hitbox` | `number` | the model radius the server gives (actor +0x208) |
| `target_index` | `integer?` | what it targets (its face target; ours: our target) |
| `target_id` | `integer?` | that target's server id, when known |
| `pet` | `boolean?` | true for our pet, which also has: |
| `mpp` | `integer?` | our pet's MP % |
| `tp` | `integer?` | our pet's TP |

#### `vana.Member`

A party or alliance member (game.party).

| Field | Type | Note |
| --- | --- | --- |
| `slot` | `integer` |  |
| `party` | `integer` |  |
| `id` | `integer` |  |
| `index` | `integer` |  |
| `name` | `string` |  |
| `hp` | `integer` |  |
| `mp` | `integer` |  |
| `tp` | `integer` |  |
| `hpp` | `integer` |  |
| `mpp` | `integer` |  |
| `zone` | `integer` |  |
| `in_zone` | `boolean` |  |
| `main_job` | `integer` |  |
| `main_level` | `integer` |  |
| `sub_job` | `integer` |  |
| `sub_level` | `integer` |  |
| `leader` | `boolean` |  |
| `alliance_leader` | `boolean` |  |
| `buffs` | `integer[]` |  |

#### `vana.Item`

An item in a bag (game.items).

| Field | Type | Note |
| --- | --- | --- |
| `bag` | `integer` |  |
| `slot` | `integer` |  |
| `id` | `integer` |  |
| `count` | `integer` |  |
| `extdata` | `string?` | 24 raw bytes |
| `locked` | `integer` |  |
| `price` | `integer?` |  |

#### `vana.Action`

An action packet (s2c 0x028) decoded: the `action` event's argument.

| Field | Type | Note |
| --- | --- | --- |
| `actor` | `integer` |  |
| `category` | `integer` |  |
| `param` | `integer` |  |
| `info` | `integer` |  |
| `targets` | `{id: integer, results: {miss: integer, kind: integer, animation: integer, info: integer, scale: integer, value: integer, message: integer, bit: integer, proc: table?, react: table?}[]}[]` |  |

#### `vana.WindowSpec`

A window's definition (ui.window).

| Field | Type | Note |
| --- | --- | --- |
| `title` | `string?` |  |
| `width` | `number?` |  |
| `help` | `string?` |  |
| `hud` | `boolean?` |  |
| `frame` | `boolean?` |  |
| `x` | `number?` |  |
| `y` | `number?` |  |
| `elements` | `table[]?` |  |

#### `vana.Packet`

A packet as a hook sees it (packets.on_in / on_out).

| Field | Type | Note |
| --- | --- | --- |
| `id` | `integer` |  |
| `data` | `string` |  |
| `original` | `string` |  |
| `injected` | `boolean` |  |
| `blocked` | `boolean` |  |
| `outgoing` | `boolean` |  |

#### `vana.KeyEvent`

A key as input.on_key sees it.

| Field | Type | Note |
| --- | --- | --- |
| `key` | `string` | a, 1, f1, enter, numpad+, lctrl... |
| `down` | `boolean` |  |
| `repeated` | `boolean` |  |
| `ctrl` | `boolean` |  |
| `alt` | `boolean` |  |
| `shift` | `boolean` |  |
| `typing` | `boolean` | a text field has the keys |
| `blocked` | `boolean` | an earlier mod took it |

#### `vana.MouseEvent`

The mouse as input.on_mouse sees it.

| Field | Type | Note |
| --- | --- | --- |
| `kind` | `'down'\|'up'\|'move'\|'wheel'` |  |
| `button` | `string?` |  |
| `x` | `number` |  |
| `y` | `number` |  |
| `delta` | `number?` |  |
| `blocked` | `boolean` |  |

#### `vana.Time`

The time (game.time): Vana'diel's date and clock, the moon, and Earth's.

| Field | Type | Note |
| --- | --- | --- |
| `earth` | `integer` |  |
| `year` | `integer` |  |
| `month` | `integer` |  |
| `day` | `integer` |  |
| `hour` | `integer` |  |
| `minute` | `integer` |  |
| `weekday` | `integer` |  |
| `moon_phase` | `integer` |  |
| `moon` | `integer` |  |

#### `vana.Mod`

The running mod (the `mod` global).

| Field | Type | Note |
| --- | --- | --- |
| `id` | `string` |  |
| `name` | `string` |  |
| `version` | `string` |  |
| `api` | `integer` |  |
| `path` | `string` |  |

<!-- End of the generated reference. -->
