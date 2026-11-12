# Getting started: your first mod

This walks you through writing a mod from an empty folder to something useful: it greets you in each zone, shows
your HP on screen, warns you when it drops, remembers where you have been, and opens a window listing who is nearby,
where picking a name targets it. Every step adds a few lines and tells you what you should see.

The finished mod is in [examples/hello](examples/hello/) if you want to compare.

You need the client and a text editor. Some Lua helps, but each step explains what its code does.

## Step 0: make the folder

Mods live in the `mods` folder of the client's data folder:

| System | Mods folder |
| --- | --- |
| Linux | `~/.local/share/vana-client/mods` |
| Windows | `%APPDATA%\vana-client\mods` |

Make a folder named `hello` there, with two empty text files in it:

```
mods/
  hello/
    mod.toml     <- describes the mod
    init.lua     <- the code; it runs when the mod loads
```

Put this in `mod.toml`:

```toml
name = "Hello"
version = "1.0.0"
description = "My first mod."
```

The folder's name, `hello`, is the mod's id. The client uses it in commands like `/lua reload hello`.

## Step 1: say something

Put one line in `init.lua`:

```lua
print("Hello is loaded!")
```

Now turn the mod on. In game, open the Config window, choose **Settings**, then the **Mods** tab. Your folder is in
the list, switched off. Switch it on, then type in the chat line:

```
/lua load hello
```

You should see `Hello is loaded!` in the chat log. From now on the mod loads by itself every time you log in.

**Tip:** type `/lua watch hello` once. After that, every time you save a file in the mod's folder, the client
reloads the mod, so you can keep the game open while you work. When you reload by hand, use `/lua reload hello`.

## Step 2: react to something

Mods mostly wait for things to happen. The client calls your functions when they do. Replace `init.lua` with:

```lua
print("Hello is loaded!")

events.on("zone_change", function(zone)
  chat.add("Welcome to " .. (res.zone(zone) or "somewhere new") .. ".")
end)
```

- `events.on(name, handler)` asks the client to call `handler` every time the event called `name` happens. Here,
  `zone_change` happens whenever you arrive in a new zone, and the client passes the new zone's number.
- `res.zone(number)` looks the zone's name up in the game's data, for example `"Port Jeuno"`. It returns nil for
  a number it does not know, hence the `or`.
- `chat.add(text)` puts a line in your chat log. Only you see it.

Save, then walk or warp into another zone. You should see `Welcome to Lower Jeuno.` (or wherever you went).

`zone_change` does not fire for the zone you log in to, only for the zones you move to after. The full list of
events is in [reference/events.md](reference/events.md).

## Step 3: a command

Add this to the end of `init.lua`:

```lua
commands.register("hp", function()
  local me = game.player()
  chat.add(("%s has %d of %d HP (%d%%)."):format(me.name, me.hp, me.hp_max, me.hpp))
end)
```

- `commands.register(name, handler)` makes a chat command. Typing `/hp` (or `//hp`) calls the handler. Words typed
  after the command are passed to it as arguments: `/hp me` would call `handler("me")`.
- `game.player()` returns a table describing your character: `name`, `hp`, `hp_max`, `hpp` (percent), `mp`, jobs,
  position and more. All its fields are listed in [reference/types.md](reference/types.md#player).
- `("%s has %d ..."):format(...)` is Lua's `string.format`: each `%s` or `%d` is replaced by the next value, and `%%`
  is a percent sign.

Type `/hp`. You should see something like `Lnxcsfix has 330 of 403 HP (81%).`

## Step 4: put something on the screen

Text in the chat log scrolls away. To keep something on the screen, use the HUD:

```lua
hud.text("hp", { x = 20, y = 200, size = 1.2, colour = { 255, 255, 255 }, bg = { 0, 0, 0, 140 } })

local function show_hp(hpp)
  hud.set("hp", { text = ("HP %d%%"):format(hpp) })
end

-- Your character is in by the time a mod loads, so this is the current value.
local hpp = game.player().hpp
if hpp then
  show_hp(hpp)
end

events.on("hpp_change", function(hpp)
  show_hp(hpp)
end)
```

- `hud.text(id, spec)` creates a text element named `"hp"`: 20 pixels from the window's left edge, 200 down, a
  little larger than normal text, white on a half-transparent black box. Colours are `{ red, green, blue, alpha }`,
  each 0 to 255. Negative positions count from the right or bottom edge.
- `hud.set(id, fields)` changes some fields of an existing element, here only its text.
- `hpp_change` fires whenever your HP percent changes, with the new value.

You should see `HP 81%` near the top left, and it changes as you take damage or heal. Everything the HUD can draw
is in [reference/hud.md](reference/hud.md).

## Step 5: remember something

Each mod gets two tables the client saves for it: `storage.account` (shared by all your characters) and
`storage.character` (one per character). Put plain data in them (strings, numbers, booleans and tables of those) and
they are written to disk when you log out, when the mod unloads, and whenever you call `storage.save()`.

Change the zone handler from step 2 to count your visits:

```lua
storage.character.visits = storage.character.visits or {}

events.on("zone_change", function(zone)
  local name = res.zone(zone) or "somewhere new"
  local key = tostring(zone)
  local visits = (storage.character.visits[key] or 0) + 1
  storage.character.visits[key] = visits
  chat.add(("Welcome to %s (visit %d)."):format(name, visits))
end)
```

The zone number is turned into a string key (`tostring(zone)`) because saved tables come back with text keys: a key
saved as the number `230` would come back as the string `"230"`, and `visits[230]` would find nothing. Use string keys
for anything indexed by an id.

Log out and back in. The counts carry on where they were.

## Step 6: a setting the player can change

Settings are declared in `mod.toml`, and the client draws them for you on the mod's page under Config → Extras. Add
this to `mod.toml`:

```toml
[settings.warn_below]
type = "int"
default = 50
min = 10
max = 90
step = 10
label = "Warn Below"
help = "Warn in the log when your HP drops below this percent."
```

and change the `hpp_change` handler from step 4:

```lua
events.on("hpp_change", function(hpp, previous)
  show_hp(hpp)
  local limit = settings.get("warn_below")
  if hpp < limit and previous >= limit then
    chat.add(("Careful: your HP is down to %d%%!"):format(hpp))
  end
end)
```

- `settings.get(key)` returns the player's value, or the `default` from mod.toml if they never changed it.
- The handler gets the previous value too, so the warning shows once, when you cross the line, not on every hit.

Reload the mod, open Config → Extras, and find **Hello**: the slider is there. Take some damage below the line to see
the warning.

## Step 7: a window, and doing something in the game

Reading the game needs no permission. Doing something in it (targeting, casting, using items) does, and the player
grants it. Add `permissions` to `mod.toml`:

```toml
permissions = ["actions"]
```

Then add a window that lists who is near you, where picking a row targets them:

```lua
local nearby = {}

ui.window("nearby", {
  title = "Nearby",
  width = 320,
  elements = {
    { type = "list", id = "who", height = 8, rows = {},
      columns = { { title = "Name" }, { title = "Kind", width = 60 }, { title = "Yalms", width = 60, align = "right" } } },
    { type = "buttons", id = "buttons", items = { "Refresh", "Close" } },
  },
})

local function refresh()
  nearby = {}
  for _, e in ipairs(game.entities()) do
    if not e.me and e.distance < 20 then
      table.insert(nearby, e)
    end
  end
  table.sort(nearby, function(a, b) return a.distance < b.distance end)
  local rows = {}
  for _, e in ipairs(nearby) do
    table.insert(rows, { e.name, e.kind, ("%.1f"):format(e.distance) })
  end
  ui.update("nearby", "who", { rows = rows })
end

ui.on("nearby", function(_, e)
  if e.kind == "select" and e.id == "who" then
    local picked = nearby[e.index]
    if picked then
      actions.target(picked.id)
    end
  elseif e.kind == "press" and e.id == "buttons" then
    if e.index == 1 then
      refresh()
    else
      ui.close("nearby")
    end
  end
end)

commands.register("nearby", function()
  refresh()
  ui.open("nearby")
end)
```

- `ui.window(id, spec)` defines a window from a list of elements: here a list with three columns, and a row of two
  buttons. The client draws it in the look of the interface you use (classic or modern), with the keys and the mouse
  working as in the game's windows. Defining it does not show it; `ui.open` does.
- `game.entities()` returns everyone and everything loaded around you: players, NPCs and monsters. Each has a
  `name`, a `kind` (`"player"`, `"npc"` or `"mob"`), a `distance` in yalms, an `id` and more.
- `ui.update(window, element, fields)` changes one element: here it fills the list's rows.
- `ui.on(window, handler)` gets what the player does in the window. Choosing a row sends `select` with the row's
  number (`e.index`, counting from 1); a button sends `press` with the button's number.
- `actions.target(id)` targets an entity by its `id`.

Reload. The mod asks for the `actions` permission: a line in the chat log says so, and the mod's Extras page has an
**Allow game actions** switch. Turn it on, then type `/nearby`. Choose a name with the arrow keys and Enter, or click
it, and your target changes.

The `actions` permission lets a mod act while it handles your input, such as a command you typed or a click in its
window. A mod that acts by itself (from a timer or an event) needs `automation` instead. See
[guide.md](guide.md#permissions).

## When something goes wrong

Make a mistake on purpose: change `me.hpp` to `me.nope` in the `/hp` command, save, and type `/hp`. The chat log shows
a red line naming the mod, the file and the line (`init.lua:24: ...`), and the command does nothing. Fix it and save
again: `/lua watch` reloads it.

More ways to find problems:

- `vana-client.log` in the client's data folder has every error in full, and whatever you write with `log.info`.
- F12 shows each mod's time per frame, memory and error count.
- `/lua run hello chat.add(tostring(game.player().hpp))` runs one line of Lua inside your mod.

A mod that keeps failing (20 errors in a minute) is suspended: it stops until you fix it and `/lua reload` it.

## Where to go next

- [guide.md](guide.md): how mods work in more depth: events and timing, waiting with `sleep`, windows, the HUD and
  world drawing, saving data, permissions, packets, talking to other mods, mods that run before login.
- [reference/](reference/README.md): every function, with its parameters, what it returns and an example.
- [examples/](examples/README.md): complete mods to read and copy.
