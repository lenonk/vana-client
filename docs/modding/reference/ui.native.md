# `ui.native`

The `ui.native` namespace works with the game's windows and HUD: which windows are open, what the player's cursor
is on, the rows of the list that has the keys, hiding parts of the HUD or the whole interface, item name colours in
shop lists, and the game's config values. The functions that read the game (`windows`, `highlighted`, `list`,
`menu_open`, `interface_hidden`, `config`) raise `game state is not available here` in the lobby and inside chat
filters. Changing things (`ui.native.interface`,
`ui.native.hide`, `ui.native.set_config`) needs the `ui.native` permission. A common pattern is to read
`ui.native.list()` or `ui.native.highlighted()` every frame and show extra information next to the game's window.

```lua
events.on("frame", function()
  local h = ui.native.highlighted()
  local it = h and h.kind == "item" and res.item(h.id)
  if it then
    hud.text("hl", {x = -10, y = 40, align = "right", text = it.name, bg = {0, 0, 0, 150}})
  else
    hud.remove("hl")
  end
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`ui.native.windows()`](#uinativewindows) | The names of the game windows open now. |
| [`ui.native.highlighted()`](#uinativehighlighted) | What the cursor is on in the game window that has the keys. |
| [`ui.native.list()`](#uinativelist) | The rows, cursor and scroll of the game list that has the keys. |
| [`ui.native.menu_open()`](#uinativemenu_open) | Whether any game window is open. |
| [`ui.native.interface_hidden()`](#uinativeinterface_hidden) | Whether the player has hidden the interface. |
| [`ui.native.interface(show)`](#uinativeinterface) | Shows or hides the whole interface. |
| [`ui.native.hide(piece, hidden?)`](#uinativehide) | Hides or shows a piece of the game's HUD. |
| [`ui.native.hidden(piece)`](#uinativehidden) | Whether any mod hides a HUD piece. |
| [`ui.native.item_colour(item, colour?)`](#uinativeitem_colour) | Colours an item's name in shop and auction house lists. |
| [`ui.native.config(id)`](#uinativeconfig) | Reads a game config value. |
| [`ui.native.set_config(id, value)`](#uinativeset_config) | Sets a game config value, as the Config window does. |

## Window names

`ui.native.windows()` names the open windows with these strings, always in this order:

| Name | Open while |
| --- | --- |
| `"menu"` | The main menu is open. |
| `"confirm"` | A yes / no question is showing. |
| `"items"` | The item list (or a storage view) or the equipment screen is open. |
| `"action"` | The battle command menu is open. |
| `"config"` | The Config window is open. |
| `"settings"` | The client's settings screen is open. |
| `"mod window"` | A mod's window has the keys. |
| `"map"` | The map is open. |
| `"fulllog"` | The full chat log is open. |
| `"magic"` | The magic list is open. |
| `"ability"` | The ability list is open (also the mount and emote lists). |
| `"cats"` | The magic or ability category menu is open. |
| `"moogle"` | A moogle's menu is open. |
| `"mogdoor"` | The Mog House's exit menu is open. |
| `"shop"` | A shop (an NPC's or a guild's) or a bazaar window is open. |
| `"auction"` | The auction house is open. |
| `"trade"` | A trade window is open. |
| `"post"` | The Delivery Box is open. |
| `"loot"` | The treasure pool window is open. |
| `"synth"` | Synthesis is open. |
| `"search"` | Search results are showing. |
| `"inspect"` | The window of a player you checked is open. |
| `"prompt"` | The target prompt is up (choosing a target for a spell, an item and the like). |
| `"typing"` | The chat line is open. |
| `"log_active"` | The log window has the focus. |
| `"ko_open"` | The K.O. window is open. |
| `"raise"` | A raise offer is waiting for an answer. |
| `"event"` | An event (a cutscene, an NPC's talk) is under way. |
| `"choice"` | An event's menu of choices is showing. |
| `"palette"` | The macro palette is open. |
| `"status"` | The Status window is open. |
| `"missions"` | The Missions window is open. |
| `"quests"` | The Quests window is open. |
| `"keyitems"` | The Key Items window is open. |
| `"linkshell"` | The Linkshell window is open. |
| `"helpdesk"` | The Help Desk is open. |

## HUD pieces

`ui.native.hide` and `ui.native.hidden` take these names. A hidden piece stays hidden in both UIs.

| Piece | What it hides |
| --- | --- |
| `"party"` | The party list and the alliance lists, with their members' status icons. |
| `"alliance"` | The alliance lists only. |
| `"target"` | The target window. |
| `"buffs"` | Your own status icons. |
| `"compass"` | The compass (in the modern UI, the location panel). |
| `"minimap"` | The minimap. |
| `"cast"` | The cast bar. |
| `"plates"` | The name plates over characters. |
| `"log"` | The log windows. They still come back while an event, a prompt or the chat line needs them. |
| `"timers"` | The Timers window. |
| `"regime"` | The training regime window. |
| `"network"` | The connection panel. |
| `"palette"` | The macro palette's drawing (its keys still run macros). |
| `"clock"` | The Earth time line of the Clock extra. |
| `"experience"` | The experience bar on the modern party window's top edge. |

## `ui.native.windows`

```lua
ui.native.windows() -> string[]
```

The names of the game's windows open now. Use it to react to a window opening, or to stay out of the way while one is
up. See [Window names](#window-names) for every name.

**Returns** `string[]`: the names of the open windows, in the order of the [Window names](#window-names) table; an
empty table when none is open.

**Errors** In the lobby or a chat filter: `game state is not available here ...`.

**Example**

```lua
commands.register("wins", function()
  local open = ui.native.windows()
  print(#open == 0 and "no windows" or table.concat(open, ", "))
end)
```

## `ui.native.highlighted`

```lua
ui.native.highlighted() -> table?
```

What the cursor is on in the game window that has the keys: a spell in the magic list, an ability, a mount or emote,
a key item, or an item (in the item list, the equipment screen, a shop, the auction house, a trade and other windows
that show an item's details). Use it to show more about the thing the player is looking at.

**Returns** `table` or `nil`: `nil` when the cursor is on nothing of these kinds. The table's fields:

| Field | Type | Description |
| --- | --- | --- |
| `window` | `string` | Where the cursor is: `"magic"`, `"ability"`, `"keyitems"`, `"items"`; for an item in another window, the first of `"shop"`, `"auction"`, `"trade"`, `"post"`, `"loot"`, `"inspect"`, `"synth"`, `"moogle"`, `"event"` that is open, else `"info"`. |
| `kind` | `string` | What `id` is: `"spell"` (a spell id), `"ability"` (an ability id as `res.ability` takes it), `"mount"` (a mount id as `res.mount` takes it), `"emote"` (an emote id), `"key_item"` (a key item id) or `"item"` (an item id). |
| `id` | `integer` | The thing's id. |
| `bag` | `integer?` | For an item in the item list or on the equipment screen: its bag (0 the inventory, as `game.items` numbers them). `nil` otherwise. |
| `slot` | `integer?` | With `bag`: its slot in that bag. |

**Errors** In the lobby or a chat filter: `game state is not available here ...`.

**Example**

```lua
commands.register("what", function()
  local h = ui.native.highlighted()
  if not h then print("nothing highlighted") return end
  print(h.window, h.kind, h.id, h.bag, h.slot)
end)
```

## `ui.native.list`

```lua
ui.native.list() -> table?
```

The game list that has the keys: which list it is, all its rows (not just those on screen), where the cursor is and
which row is at the top. It covers the magic and ability lists, the item lists, shops and bazaars, the auction house
and the Key Items list. Read it to show extra columns or to find a row; to move the cursor, press keys with
`input.press` (which needs `input.capture`).

**Returns** `table` or `nil`: `nil` when none of these lists has the keys. The table's fields:

| Field | Type | Description |
| --- | --- | --- |
| `window` | `string` | Which list (see the table below). |
| `rows` | `table[]` | Every row, top to bottom; the fields of a row depend on the list (below). |
| `cursor` | `integer` | The row the cursor is on, from 1 (`rows[cursor]` is that row). |
| `top` | `integer?` | The row shown at the top of the list, from 1; `nil` for a menu, which does not scroll. |

The lists and their rows:

| `window` | Each row's fields |
| --- | --- |
| `"magic"` | `spell` (spell id). |
| `"ability"` | `ability` (ability id, as `res.ability` takes it); in the mount list `mount` (mount id) instead, in the emote list `emote` (emote id). |
| `"items"` | `item` (item id), `count`, `bag` (the bag shown: 0 the inventory, as `game.items` numbers them), `slot` (its slot in that bag), `equipped` (boolean). |
| `"shop buy"` | `item`, `price`; in a guild shop also `count` (its stock). |
| `"shop sell"` | `item`, `count`, `bag` (0), `slot`, `price`: at an NPC shop the appraised price (`nil` before the shop appraised it), at a guild the guild's price (`nil` for an item it does not buy). |
| `"shop wanted"` | A guild's list of what it buys. The first row is the list's heading and is an empty table; then `item`, `count`, `price`. |
| `"shop bazaar"` | Setting up your bazaar: `item`, `count`, `bag` (0), `slot`, `price` (the bazaar price you set; `nil` for an item not in your bazaar). |
| `"shop wares"` | Another player's bazaar: `item`, `count`, `price`. |
| `"auction list"` | While bidding: `item`, `stack` (true for a row of stacks), `single_count` and `stack_count` (the counts the list shows). In Sales Status: `item`, `sale_slot` (which of your sale slots), `price`. |
| `"auction history"` | An item's price history: `item`, `price`, `date` (Unix time), `seller`, `buyer`. |
| `"auction sell"` | The inventory to sell from: `item`, `count`, `bag` (0), `slot`. |
| `"auction "` and a menu name | One of the auction house's menus, for example `"auction auc1"`. `rows` is empty and `top` is `nil`; `cursor` is the highlighted entry's number in the menu, from 1. |
| `"keyitems"` | `key_item` (key item id). |

**Errors** In the lobby or a chat filter: `game state is not available here ...`.

**Example**

```lua
commands.register("rows", function()
  local l = ui.native.list()
  if not l then print("no list has the keys") return end
  print(l.window .. ": " .. #l.rows .. " rows, cursor on " .. l.cursor)
  local r = l.rows[l.cursor]
  if r and r.item then
    local it = res.item(r.item)
    print("item", it and it.name, "count", r.count)
  end
end)
```

## `ui.native.menu_open`

```lua
ui.native.menu_open() -> boolean
```

Whether any of the game's windows is open: menus, lists, shops, prompts, the battle command menu and so on, or a mod
window that has the keys. Use it to hide your HUD while the player works in a window.

**Returns** `boolean`: `true` while such a window is open. It is not the same test as `#ui.native.windows() > 0`: an
event, the chat line or the macro palette alone do not count here.

**Errors** In the lobby or a chat filter: `game state is not available here ...`.

**Example**

```lua
hud.text("tip", {x = 20, y = 20, text = "Press F9 for help"})
events.on("frame", function()
  hud.set("tip", {visible = not ui.native.menu_open()})
end)
```

## `ui.native.interface_hidden`

```lua
ui.native.interface_hidden() -> boolean
```

Whether the whole interface is hidden, as the game's UI-hide key hides it (or `ui.native.interface(false)`).

**Returns** `boolean`: `true` while the interface is hidden.

**Errors** In the lobby or a chat filter: `game state is not available here ...`.

**Example**

```lua
timer.every(1, function()
  if ui.native.interface_hidden() then log.info("interface hidden") end
end)
```

## `ui.native.interface`

```lua
ui.native.interface(show)
```

Shows or hides the whole interface, as the game's UI-hide key does. While it is hidden, nothing of the interface draws,
your mod's HUD elements included, and the player can press Escape to show it again.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `show` | `boolean` | yes | `true` shows the interface, `false` hides it. |

**Returns** nothing.

**Needs** the `ui.native` permission.

**Errors** Without the permission: `this needs the ui.native permission: ...`.

**Example**

```lua
commands.register("clean", function()
  ui.native.interface(ui.native.interface_hidden())
end)
```

The change is queued and applies after the mod phase, so `ui.native.interface_hidden()` still gives the old value
until then.

## `ui.native.hide`

```lua
ui.native.hide(piece, hidden?)
```

Hides one piece of the game's HUD (see [HUD pieces](#hud-pieces)) while your mod is loaded, for example to replace it
with your own. It shows again when your mod unloads, or when you call `ui.native.hide(piece, false)`. If several mods
hide the same piece, it stays hidden until every one of them has shown it again.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `piece` | `string` | yes | The piece's name, from the [HUD pieces](#hud-pieces) table. |
| `hidden` | `boolean` | no, default `true` | `true` hides it; `false` takes your mod's hide away. |

**Returns** nothing.

**Needs** the `ui.native` permission.

**Errors**
- Without the permission: `this needs the ui.native permission: ...`.
- A name not in the table: `no HUD piece "..."`, followed by the list of names.

**Example**

```lua
-- Replace the party list with a plain list of names and HP.
ui.native.hide("party")
hud.text("party", {x = -10, y = 200, align = "right", bg = {0, 0, 0, 140}})
events.on("frame", function()
  local lines = {}
  for _, m in ipairs(game.party()) do
    lines[#lines + 1] = m.name .. "  " .. m.hpp .. "%"
  end
  hud.set("party", {text = table.concat(lines, "\n")})
end)
```

## `ui.native.hidden`

```lua
ui.native.hidden(piece) -> boolean
```

Whether any mod (yours or another) hides a HUD piece now.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `piece` | `string` | yes | The piece's name, from the [HUD pieces](#hud-pieces) table. |

**Returns** `boolean`: `true` when at least one mod hides it; `false` otherwise, and for a name not in the table.

**Example**

```lua
if ui.native.hidden("minimap") then
  print("another mod hides the minimap")
end
```

## `ui.native.item_colour`

```lua
ui.native.item_colour(item, colour?)
```

Colours an item's name in the game's shop and auction house lists while your mod is loaded, for example to mark items
worth buying. Leave `colour` out (or pass `nil`) to give the item its usual colour back. When several mods colour the
same item, the last call wins.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `item` | `integer` | yes | The item id. |
| `colour` | `{r, g, b, a}` | no | The colour (0..255 each, `a` defaults to 255). `nil`, or anything that is not a table, takes your colour away (another mod's colour for the item is left alone). |

**Returns** nothing.

**Example**

```lua
local wanted = {4096, 4097, 4098}
for _, id in ipairs(wanted) do
  ui.native.item_colour(id, {120, 255, 120})
end
```

The colours go away when your mod unloads.

## `ui.native.config`

```lua
ui.native.config(id) -> integer
```

Reads one of the game's config values by its id: the settings of the game's Config window.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The config id (0 to 65535). |

**Returns** `integer`: the setting's value (its default when the player never changed it); 4294967295 for an id that
is no setting.

**Errors** In the lobby or a chat filter: `game state is not available here ...`.

**Example**

```lua
commands.register("cfg", function(id)
  local n = tonumber(id)
  if n then print("config " .. n .. " = " .. ui.native.config(n)) end
end)
```

## `ui.native.set_config`

```lua
ui.native.set_config(id, value)
```

Sets one of the game's config values and applies it, as the Config window does, and saves it with the game's
settings.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The config id (0 to 65535). |
| `value` | `integer` | yes | The new value: 32 bits, so -2147483648 to 4294967295 (a negative value for a signed setting). A value outside the setting's range sets its default instead. |

**Returns** nothing.

**Needs** the `ui.native` permission.

**Errors** Without the permission: `this needs the ui.native permission: ...`; `config value <n>: 32 bits (-2147483648..4294967295)` for a value that does not fit.

**Example**

```lua
commands.register("setcfg", function(id, value)
  local i, v = tonumber(id), tonumber(value)
  if i and v then ui.native.set_config(i, v) end
end)
```

The change is queued and applies after the mod phase, so `ui.native.config` gives the old value until then. An id
that is no setting is ignored.
