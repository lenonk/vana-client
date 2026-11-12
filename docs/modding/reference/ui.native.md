# `ui.native`

The `ui.native` namespace works with the game's windows and HUD: which windows are open, what the player's cursor
is on, the rows of the list that has the keys, hiding parts of the HUD or the whole interface, moving, scaling and
fading the modern UI's panels and finding where they are, item name colours in shop lists, and the game's config
values. To change how the modern UI looks (its sprites, colours and frames), see [ui.skin](ui.skin.md). The functions that read the game (`windows`, `highlighted`, `list`,
`menu_open`, `interface_hidden`, `config`) raise `game state is not available here` in the lobby and inside chat
filters. Changing things (`ui.native.interface`,
`ui.native.hide`, `ui.native.place`, `ui.native.set_config`) needs the `ui.native` permission. A common pattern is to read
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
| [`ui.native.battle_menu()`](#uinativebattle_menu) | The modern battle menu's cursor on its suggestion rows. |
| [`ui.native.interface_hidden()`](#uinativeinterface_hidden) | Whether the player has hidden the interface. |
| [`ui.native.interface(show)`](#uinativeinterface) | Shows or hides the whole interface. |
| [`ui.native.hide(piece, hidden?)`](#uinativehide) | Hides or shows a piece of the game's HUD, or a modern panel. |
| [`ui.native.hidden(piece)`](#uinativehidden) | Whether any mod hides a HUD piece or panel. |
| [`ui.native.slot(slot, value?)`](#uinativeslot) | Fills a built-in panel's slot: the clock line, the target's distance and HP %. |
| [`ui.native.place(panel, place?)`](#uinativeplace) | Moves, scales or fades a modern panel. |
| [`ui.native.rect(panel)`](#uinativerect) | Where a modern panel is on screen. |
| [`ui.native.panels()`](#uinativepanels) | Every modern panel a mod may name, with where it is. |
| [`ui.native.item_colour(item, colour?)`](#uinativeitem_colour) | Colours an item's name in shop and auction house lists. |
| [`ui.native.trim(list, ids?)`](#uinativetrim) | Leaves spells, abilities or mounts out of the game's lists. |
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
| `"buffs"` | Your status icons. |
| `"compass"` | The compass (in the modern UI, the location panel). |
| `"minimap"` | The minimap. |
| `"cast"` | The cast bar. |
| `"plates"` | The name plates over characters. |
| `"log"` | The log windows. They still come back while an event, a prompt or the chat line needs them. |
| `"timers"` | The Timers window. |
| `"regime"` | The training regime window. |
| `"network"` | The connection panel. |
| `"palette"` | The macro palette's drawing (its keys still run macros). |
| `"clock"` | The line beside the game's clock ([`ui.native.slot`](#uinativeslot)'s `clock`). |
| `"experience"` | The experience bar on the modern party window's top edge. |

## Modern panels

In the modern UI the HUD and the menus are panels, each with a name. `ui.native.hide` takes the names marked
"hides" (none of them takes the keys), `ui.native.place` those marked "moves" (the edge windows stay on their edge),
and `ui.native.rect`, `ui.native.panels` and a HUD element's `anchor` take every name. In the classic UI only the
[HUD pieces](#hud-pieces) hide, and `ui.native.rect` knows only `"chat"` (the log window); the rest of this works in
the modern UI.

| Panel | Hides | Moves | What it is |
| --- | --- | --- | --- |
| `"chat"` | yes | yes | The chat window: the log and the chat line (back while an event, a prompt or the chat line needs it). |
| `"party"` | yes | yes | The party window. |
| `"alliance1"`, `"alliance2"` | yes | yes | The alliance's other parties. |
| `"target"` | yes | yes | The target window. |
| `"focus"` | yes | yes | The focus target and the last target. |
| `"status"` | yes | yes | Your status icons. |
| `"where"` | yes | yes | The location panel: zone, time, day, weather, compass. |
| `"cast"` | yes | yes | The cast bar. |
| `"pending"` | yes | yes | The chip for an action waiting for the server's answer. |
| `"countdown"`, `"objective"` | yes | yes | An event's countdown and objective bars. |
| `"combat"` | yes | yes | The battle menu while engaged. |
| `"favourites"` | yes | yes | The favourites strip. |
| `"iteminfo"` | yes | yes | The item details beside the item lists. |
| `"command"` | no | yes | The main menu. |
| `"window"` | no | yes | The main menu's windows (status, equipment, ...). |
| `"list"`, `"cats"` | no | yes | The magic and ability lists, and their categories. |
| `"loot"` | no | yes | The treasure pool. |
| `"check"` | no | yes | A player's /check. |
| `"timers"`, `"minimap"`, `"regime"` | yes | no | The edge windows: Timers, the minimap, the training regime window. |

**Replacing a panel.** Hide it, then draw your HUD elements with `anchor` set to its name: they stand where the
panel was (its last rect). **Adding to a panel.** HUD elements draw under the panels, so an element anchored to a
shown panel goes beside it (`{anchor = "target", y = -24}` above the target window), and follows it as the player
moves it. The panel's place is the player's
to set; `ui.native.place` overrides it while your mod runs.

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

## `ui.native.battle_menu`

```lua
ui.native.battle_menu() -> table?
```

The modern battle menu while it shows its commands (no target prompt over it), as a mod filling its `battle_rows`
[slot](#uinativeslot) needs it: which of those rows the cursor is on, and that row's submenu.

**Returns** `table?`: `nil` while the menu is not up (or in the classic UI); otherwise `{row, sub}`: `row` the
`battle_rows` row under the cursor (1-based; nil when the cursor is on the commands), `sub` the cursor in that row's
open submenu (1-based; nil when it is not open).

**Example**

```lua
local m = ui.native.battle_menu()
if m and m.row then print("on suggestion " .. m.row) end
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

Hides one piece of the game's HUD (see [HUD pieces](#hud-pieces)), or in the modern UI a [panel](#modern-panels)
by name, while your mod is loaded, for example to replace it with yours. It shows again when your mod unloads, or
when you call `ui.native.hide(piece, false)`. If several mods hide the same piece, it stays hidden until every one of
them has shown it again. A piece and a panel of the same name are one: `"party"` (the piece) hides the alliance
parties too, `"alliance1"` only that one.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `piece` | `string` | yes | The piece's name, from the [HUD pieces](#hud-pieces) table, or a panel that hides, from the [Modern panels](#modern-panels) table. |
| `hidden` | `boolean` | no, default `true` | `true` hides it; `false` takes your mod's hide away. |

**Returns** nothing.

**Needs** the `ui.native` permission.

**Errors**
- Without the permission: `this needs the ui.native permission: ...`.
- A name in neither table, or a panel that takes the keys: `no HUD piece or panel "..." that can hide`, followed by
  the list of names.

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

## `ui.native.slot`

```lua
ui.native.slot(slot, value?)
```

Puts your values in a slot of a built-in panel. The panel draws them as its text, in both UIs, in its font,
place and colours, so they look as the rest of it does. The value stays until you change it, empty it with `nil`, or
your mod unloads. If two mods fill one slot, the last call wins; a mod's `nil` only empties a slot it filled.

| Slot | Value | Where it shows |
| --- | --- | --- |
| `"clock"` | `{earth, moon_name, moon, next_day, next_minutes}`, the table [`game.clock`](game.md#gameclock) returns | Beside the game's clock: Earth time, the moon's percent and the next day (the modern location panel puts it on one line, the moon's name in its tooltip). |
| `"target_distance"` | `number`, in yalms | The target window: left of the HP gauge in the classic UI (targets with a gauge), beside the name in the modern one. Shown with one decimal. |
| `"target_hpp"` | `number`, a percent | The classic target window's right edge, on the gauge's row, in the HP colour. The modern one always shows the percent. |
| `"battle_rows"` | a list of up to 8 rows | The modern battle menu, above its commands, as the menu's rows: the action's icon and name (or `label`), `reason` in its tooltip. The cursor rides them with the commands. Enter or a click picks the action through Magic or Abilities and its list, as the player would, and the target prompt starts on `target` (a party member's name) when given; a row with a `group` (spell ids) opens a submenu of them with Right or Enter. Each row: `{kind = "spell" or "ability", id, target, reason, label, group}` (`id` a spell id or a `res.ability` id). The [`suggestions`](events.md#suggestions) event's rows come in this shape. Not drawn in the classic UI. |

The target slots show on whatever is targeted, so set them as the target changes: every frame is simplest.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `slot` | `string` | yes | `"clock"`, `"target_distance"`, `"target_hpp"` or `"battle_rows"`. |
| `value` | see above | no | `nil` (or `false`) empties the slot. |

**Returns** nothing.

**Needs** the `ui.native` permission.

**Errors**
- Without the permission: `this needs the ui.native permission: ...`.
- An unknown slot: `no slot "..."`, with the list; a value of the wrong shape: `slot ...: the wrong shape of value`.

**Example**

```lua
-- The bundled Target Info's way: the distance with every frame.
events.on("frame", function()
  local t = game.target()
  ui.native.slot("target_distance", t and t.distance or nil)
end)
```

## `ui.native.place`

```lua
ui.native.place(panel, place?)
```

Moves, scales or fades one of the modern UI's [panels](#modern-panels) while your mod is loaded. Given an `x` or a
`y`, the panel stands there and the player's drag no longer moves it (its place stays saved for when your mod
gives it back). A `scale` grows or shrinks it about its `origin`, the corner that stays put; clicks and the wheel work
on it at that size. A panel at the right or bottom of the screen grows best from its bottom right.
`alpha` fades it (0 does not draw it at all). Call it again with other values to change them, and with `nil` to give
the panel back. If two mods place one panel, the last call wins; a mod's `nil` only takes away its place.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `panel` | `string` | yes | A panel that moves, from the [Modern panels](#modern-panels) table. |
| `place` | `table` | no | The place; `nil` gives the panel back. |

**`place` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `x` | `number` | the panel's | Its left edge in window pixels; negative counts from the right edge (`-400`: 400 pixels in from it). |
| `y` | `number` | the panel's | Its top edge in window pixels; negative counts from the bottom edge. |
| `scale` | `number` | `1` | Its size, 0.25 to 4. |
| `origin` | `string` | `"top_left"` | The point it scales about: `"top_left"`, `"top_right"`, `"bottom_left"`, `"bottom_right"` or `"center"`. |
| `alpha` | `number` | `1` | Its opacity, 0 to 1. |

**Returns** nothing.

**Needs** the `ui.native` permission.

**Errors**
- Without the permission: `this needs the ui.native permission: ...`.
- An unknown name: `no panel "..."`, with the list; an edge window: `... stays on its edge: it can be hidden, not
  placed`.

**Example**

```lua
-- A bigger target window, a little see-through, growing up and left from where it stands.
ui.native.place("target", {scale = 1.4, origin = "bottom_right", alpha = 0.85})
-- Or at a point you choose: 100 pixels in from the left, 200 down.
ui.native.place("target", {x = 100, y = 200, scale = 1.4})
```

## `ui.native.rect`

```lua
ui.native.rect(panel) -> table?
```

Where one of the modern UI's [panels](#modern-panels) was drawn last frame, in window pixels (as `hud` places
elements). A panel that is not drawn now (hidden by a mod, faded out by a UI mode, or closed) keeps the rect it last
had, with `shown = false`. A panel a mod hides has the place it would stand at now, at its last size (panels that
sit on another move with it), with no height if it has never been drawn: anchor to such a panel by the corner its place
keeps (the location panel stands on the chat window, so its bottom).

The chat window grows with its lines and folds into its bottom edge when idle, and what stands on it moves with it.
Its rect also has `room`, `{x, y, w, h}`: where it stands at its full height. Place something over `room.y` to keep it
still while the chat window grows and folds (the Hotbars mod does).

In the classic UI only `"chat"` has a rect: the log window, with log window 2 when that stands over or beside it, and
its `room` at the most lines the windows open to.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `panel` | `string` | yes | A name from the [Modern panels](#modern-panels) table. |

**Returns** `table?`: `{name, x, y, w, h, shown}` (and `room` for the chat window), or `nil` when the panel has been neither drawn nor hidden since
login, and in the classic UI for every panel but `"chat"`.

**Errors** An unknown name: `no panel "..."`, with the list.

**Example**

```lua
-- A line of text just over the target window, wherever the player put it.
events.on("frame", function()
  local r = ui.native.rect("target")
  if r and r.shown then
    hud.text("over_target", {x = r.x, y = r.y - 22, text = "Engage!"})
  else
    hud.remove("over_target")
  end
end)
```

For text that only follows a panel, an element's `anchor` (see [hud](hud.md#places-sizes-and-layers)) does the
same without a `frame` handler.

## `ui.native.panels`

```lua
ui.native.panels() -> table[]
```

Every panel of the [Modern panels](#modern-panels) table, in its order: `name`, `about` (what it is), `hideable`,
`movable`, `shown`, `placed_by` (the id of the mod placing it, or `nil`), and `x`, `y`, `w`, `h` once it has been
drawn.

**Returns** `table[]`, never nil.

**Example**

```lua
for _, p in ipairs(ui.native.panels()) do
  if p.shown then print(p.name, p.x, p.y, p.w, p.h) end
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

## `ui.native.trim`

```lua
ui.native.trim(list, ids?)
```

Leaves rows out of one of the game's lists while your mod is loaded, in both UIs and in the list search: spells you
never cast, abilities you never use, mounts you never ride. Only the lists change: a command or macro still uses them.
Each call replaces your mod's rows for that list; `nil` or an empty table gives them all back. Rows left out by any
mod stay out.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `list` | `string` | yes | `"magic"` (spell ids), `"abilities"` (ids as `res.ability` takes them: weapon skills, job abilities, pet commands) or `"mounts"` (mount ids, as `res.mount`). |
| `ids` | `integer[]` | no | The rows to leave out. |

**Returns** nothing.

**Needs** the `ui.native` permission.

**Errors** `no list "<name>" (magic, abilities, mounts)`.

**Example**

```lua
-- Raise and Raise II out of the magic list.
ui.native.trim("magic", { 12, 13 })
```

The lists open whole again when your mod unloads.

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
