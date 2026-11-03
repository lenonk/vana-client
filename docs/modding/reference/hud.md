# `hud`

The `hud` namespace draws things over the game: texts, filled rectangles, bars, a map, PNG images and item or status
icons. Each element has an id you choose. You create it once (`hud.text`, `hud.rect`, ...), change it with `hud.set`
as things happen, and it stays on screen until you remove it; you do not redraw it every frame. HUD elements draw over
the game world and the name plates and under every window, in the game world only, and they hide while the player has
the interface hidden. None of these functions needs a permission. A click on an element can be handled with `hud.on`.

```lua
hud.text("hp", {x = 20, y = -80, text = "HP", size = 1.5, colour = {255, 220, 120}})
events.on("frame", function()
  local me = game.player()
  if me.hpp then hud.set("hp", {text = "HP " .. me.hpp .. "%"}) end
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`hud.text(id, spec)`](#hudtext) | Creates or replaces a text. |
| [`hud.rect(id, spec)`](#hudrect) | Creates or replaces a filled rectangle. |
| [`hud.bar(id, spec)`](#hudbar) | Creates or replaces a bar filled from 0 to 1. |
| [`hud.map(id, spec)`](#hudmap) | Creates or replaces a window onto the zone's map. |
| [`hud.image(id, spec)`](#hudimage) | Creates or replaces a PNG image from the mod's folder. |
| [`hud.icon(id, spec)`](#hudicon) | Creates or replaces an item's or a status effect's icon. |
| [`hud.set(id, fields)`](#hudset) | Changes some fields of an element. |
| [`hud.remove(id)`](#hudremove) | Removes an element. |
| [`hud.clear()`](#hudclear) | Removes every element of the mod. |
| [`hud.on(id, handler)`](#hudon) | Sets the function called when the player clicks an element. |
| [`hud.screen()`](#hudscreen) | The game window's size in pixels, and the UI's scale. |
| [`hud.bounds(id)`](#hudbounds) | Where an element was drawn last frame. |

## Places, sizes and layers

**Places.** Every element has `x` and `y`, in pixels of the game window, from its top left corner (y goes down). A
negative value counts from the right or the bottom edge instead: `x = -200` puts the element's left edge 200 pixels
left of the window's right edge, and `y = -50` puts its top 50 pixels above the bottom. For a text, `align` says
which part of it sits at `x`.

**Sizes.** The `w` and `h` of rectangles, bars, maps and images, and the `size` of icons, are in window pixels too, so
they stay the same number of pixels whatever the UI's scale. A text's `size` is different: it is a multiple of the UI's
normal text size, so texts grow and shrink with the UI. To size other elements in step with the UI, multiply by
`hud.screen().ui_scale` (pixels per UI unit):

```lua
local s = hud.screen().ui_scale
hud.rect("panel", {x = 20, y = 200, w = 160 * s, h = 40 * s, colour = {0, 0, 0, 140}})
hud.text("panel_text", {x = 20 + 8 * s, y = 200 + 10 * s, text = "Scaled with the UI"})
```

**Fields every element has:**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `x` | `number` | `0` | Its left edge (for a text, see `align`), in window pixels; negative counts from the right edge. |
| `y` | `number` | `0` | Its top edge, in window pixels; negative counts from the bottom edge. |
| `visible` | `boolean` | `true` | `false` hides it (it keeps its fields, and takes no clicks while hidden). |
| `layer` | `integer` | `0` | Elements on higher layers draw over those on lower ones. On the same layer, mods draw in their load order, and each mod's elements in the order it created them. |

**Create and set.** Each create call (`hud.text`, `hud.rect`, ...) makes a new element from its spec: fields you leave
out take their defaults. If your mod already has an element with that id, of any kind, it is replaced, keeping its
place in the drawing order. `hud.set` changes only the fields you give. Changes apply at once (they show from the next
frame drawn).

**Colours** are `{r, g, b, a}` tables with values 0 to 255, by position (`{255, 0, 0}`) or by name
(`{r = 255, g = 0, b = 0}`); a missing channel is 255. Anything else than a table raises
`a colour is {r, g, b, a} (0..255)`.

**Clicks.** An element takes clicks only once you give it a handler with [`hud.on`](#hudon).

The player's `/lua hide` command hides every mod's HUD elements (the mods keep running).

## `hud.text`

```lua
hud.text(id, spec)
```

Creates or replaces a text on the screen: one line or several, in a colour and a size you choose, with an optional box,
outline or shadow behind it. Use it for counters, labels and short messages.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The element's id, unique among your mod's HUD elements. |
| `spec` | `table` | yes | The text's fields. |

**`spec` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `x`, `y`, `visible`, `layer` | | | As for [every element](#places-sizes-and-layers). |
| `text` | `string` | `""` | The text. `"\n"` starts a new line. |
| `size` | `number` | `1` | The size as a multiple of the UI's normal text (0.25 to 8). Each line is 16 UI units tall times `size`. |
| `colour` | `{r, g, b, a}` | white | The text's colour. |
| `bg` | `{r, g, b, a}` | none | A filled box behind the text. |
| `pad` | `number` | `4` | How far the `bg` box reaches beyond the text: `pad` at the left and the right, half of it above and below (UI units, at least 0). |
| `align` | `string` | `"left"` | Which part of the text sits at `x`: `"left"` its left edge, `"center"` (or `"centre"`) its middle, `"right"` its right edge. Any other value means left. |
| `font` | `string` | the UI's | `"game"` for the game's font, `"regular"` or `"bold"` for the modern UI's faces (in either UI). Any other value, or none, uses the font of the UI the player has chosen. |
| `outline` | `{r, g, b, a}` | none | An outline of this colour round the letters. |
| `outline_width` | `number` | `1` | The outline's thickness (0.5 to 4; it grows with `size` above 1). |
| `shadow` | `boolean` | `false` | A dark drop shadow under the text. |

**Returns** nothing.

**Errors** A colour field that is not a table: `a colour is {r, g, b, a} (0..255)`. A field of the wrong kind raises a
conversion error.

**Example**

```lua
hud.text("where", {
  x = -10, y = 10, align = "right",
  text = "Zone " .. tostring(game.zone()),
  bg = {0, 0, 0, 150}, outline = {0, 0, 0}, font = "bold",
})
```

## `hud.rect`

```lua
hud.rect(id, spec)
```

Creates or replaces a filled rectangle. Use it as a panel behind other elements, a marker or a clickable button.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The element's id. |
| `spec` | `table` | yes | The rectangle's fields. |

**`spec` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `x`, `y`, `visible`, `layer` | | | As for [every element](#places-sizes-and-layers). |
| `w` | `number` | `100` | Its width in window pixels. |
| `h` | `number` | `20` | Its height in window pixels. |
| `colour` | `{r, g, b, a}` | `{0, 0, 0, 160}` | Its colour (a translucent black by default). |

**Returns** nothing.

**Errors** A `colour` that is not a table: `a colour is {r, g, b, a} (0..255)`.

**Example**

```lua
hud.rect("flash", {x = 0, y = 0, w = hud.screen().width, h = 6, colour = {255, 40, 40, 200}})
timer.after(0.5, function() hud.remove("flash") end)
```

## `hud.bar`

```lua
hud.bar(id, spec)
```

Creates or replaces a bar: a dark box filled from the left as far as `value` (0 to 1). Use it for HP, a timer or any
progress.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The element's id. |
| `spec` | `table` | yes | The bar's fields. |

**`spec` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `x`, `y`, `visible`, `layer` | | | As for [every element](#places-sizes-and-layers). |
| `w` | `number` | `120` | Its width in window pixels. |
| `h` | `number` | `8` | Its height in window pixels. |
| `value` | `number` | `0` | How full it is, 0 to 1 (values outside are cut to that range). |
| `kind` | `string` | `"hp"` | The fill's colour by kind: `"hp"` green, `"mp"` orange, `"tp"` blue, `"cast"` cyan, `"experience"` violet, `"recast"` grey. Any other value gives the `"hp"` green. |
| `colour` | `{r, g, b, a}` | none | A fill colour of your own, used instead of `kind`'s. |

**Returns** nothing.

**Errors** A `colour` that is not a table: `a colour is {r, g, b, a} (0..255)`.

**Example**

```lua
hud.bar("target_hp", {x = -220, y = 60, w = 200, h = 10})
events.on("frame", function()
  local t = game.target()
  hud.set("target_hp", {visible = t ~= nil, value = t and t.hpp / 100 or 0})
end)
```

## `hud.map`

```lua
hud.map(id, spec)
```

Creates or replaces a window onto the map of the place the player is in: the same map the client shows there (see
`game.map`). By default it is centred on the player and follows them. Where the client has no map for the place,
nothing is drawn.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The element's id. |
| `spec` | `table` | yes | The map's fields. |

**`spec` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `x`, `y`, `visible`, `layer` | | | As for [every element](#places-sizes-and-layers). |
| `w` | `number` | `200` | Its width in window pixels. |
| `h` | `number` | `200` | Its height in window pixels. |
| `centre` | `{x, z}` | the player's position | A world position (both `x` and `z` required) to centre the map on, instead of following the player. |
| `span` | `number` | `256` | How many map pixels show across the element's width (16 to 2048). The map image is 512 map pixels wide, so 512 shows the whole map across. |
| `alpha` | `number` | `1` | Its opacity, 0 to 1. |

**Returns** nothing.

**Errors** A `centre` table without a number `x` or `z` raises a conversion error.

**Example**

This draws the map and a dot for the target over it. One world unit is `px_per_unit` map pixels (from `game.map`),
and one map pixel is `w / span` window pixels; world `z` grows upward on the map.

```lua
local MX, MY, W, H, SPAN = 20, 300, 200, 200, 256
hud.map("map", {x = MX, y = MY, w = W, h = H, span = SPAN})
hud.rect("dot", {w = 6, h = 6, colour = {255, 60, 60}, layer = 1})
events.on("frame", function()
  local m, me, t = game.map(), game.player(), game.target()
  if not (m and t) then hud.set("dot", {visible = false}) return end
  local k = m.px_per_unit * W / SPAN
  hud.set("dot", {visible = true,
    x = MX + W / 2 + (t.pos.x - me.pos.x) * k - 3,
    y = MY + H / 2 - (t.pos.z - me.pos.z) * k - 3})
end)
```

## `hud.image`

```lua
hud.image(id, spec)
```

Creates or replaces an image from a PNG file in your mod's folder. The file is read when you call this, so a missing
or broken file is an error at once.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The element's id. |
| `spec` | `table` | yes | The image's fields. |

**`spec` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `x`, `y`, `visible`, `layer` | | | As for [every element](#places-sizes-and-layers). |
| `path` | `string` | (required) | The PNG's path inside the mod's folder, with `/` between folders, e.g. `"img/frame.png"`. It cannot leave the folder: no `..`, no leading `/`, no `:`. |
| `w` | `number` | the image's width | Its drawn width in window pixels. |
| `h` | `number` | the image's height | Its drawn height in window pixels. |
| `colour` | `{r, g, b, a}` | white | A tint the image's colours are multiplied by; `a` fades it. |

**Returns** nothing.

**Errors**
- No `path`: a conversion error.
- A path that would leave the mod's folder: `"...": outside the mod's folder`.
- A file that cannot be read: the path and the system's reason.
- A file that is not a PNG: the path, the reason and `(a PNG)`.
- A `colour` that is not a table: `a colour is {r, g, b, a} (0..255)`.

**Example**

```lua
hud.image("logo", {x = -74, y = -74, path = "img/logo.png", w = 64, h = 64, colour = {255, 255, 255, 200}})
```

`hud.set` cannot change an image's `path`: call `hud.image` again with the new one.

## `hud.icon`

```lua
hud.icon(id, spec)
```

Creates or replaces a square icon: an item's icon or a status effect's icon, by id.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The element's id. |
| `spec` | `table` | yes | The icon's fields. |

**`spec` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `x`, `y`, `visible`, `layer` | | | As for [every element](#places-sizes-and-layers). |
| `icon` | `string` | status | `"item"` for an item's icon; any other value, or none, for a status effect's icon. |
| `id` | `integer` | `0` | The item id or the status effect id. |
| `size` | `number` | `32` | Its width and height in window pixels (at least 1). |

**Returns** nothing.

**Example**

```lua
local x = 20
for _, b in ipairs(game.buffs()) do
  hud.icon("buff" .. b.id, {x = x, y = 120, id = b.id, size = 24})
  x = x + 26
end
```

## `hud.set`

```lua
hud.set(id, fields)
```

Changes some fields of one of your mod's HUD elements; the others keep their values. The fields are those of the
element's create call (`hud.text`, `hud.bar`, ...); fields that belong to other kinds are ignored. An element's kind
cannot change: create it again for that.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The element's id. |
| `fields` | `table` | yes | The fields to change. |

**Returns** nothing.

**Errors** No element with that id: `no HUD element "..."`. Fields of the wrong kind raise the same errors as the
create call.

**Example**

```lua
hud.bar("cast", {x = 300, y = -120, w = 200, h = 8, kind = "cast"})
events.on("frame", function()
  local c = game.cast()
  hud.set("cast", {visible = c ~= nil, value = c and c.progress or 0})
end)
```

## `hud.remove`

```lua
hud.remove(id)
```

Removes one of your mod's HUD elements. An id with no element is ignored. A click handler set for the id with `hud.on`
stays, and works again if you create an element with that id later.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The element's id. |

**Returns** nothing.

**Example**

```lua
hud.text("note", {x = 20, y = 20, text = "Saved."})
timer.after(3, function() hud.remove("note") end)
```

## `hud.clear`

```lua
hud.clear()
```

Removes every HUD element of your mod (other mods' elements stay). Click handlers set with `hud.on` stay registered.

**Returns** nothing.

**Example**

```lua
commands.register("hudoff", function()
  hud.clear()
end)
```

## `hud.on`

```lua
hud.on(id, handler)
```

Sets the function called when the player clicks one of your HUD elements with the left or the right mouse button. The
press then belongs to your mod: the game sees neither it nor its release, and `input.on_mouse` filters do not see the
press. An element has one handler: a second call replaces the first. You may set it before the element exists.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The element's id. |
| `handler` | `function` | yes | Called as `handler(id, event)` when the button goes down on the element. |

`event` is a table:

| Field | Type | Description |
| --- | --- | --- |
| `kind` | `string` | Always `"click"`. |
| `id` | `string` | The element's id. |
| `button` | `string` | `"left"` or `"right"`. |

**Returns** nothing.

**Example**

```lua
local s = hud.screen().ui_scale
hud.rect("heal", {x = 20, y = 240, w = 80 * s, h = 24 * s, colour = {40, 40, 90, 220}})
hud.text("heal_label", {x = 20 + 40 * s, y = 240 + 4 * s, text = "Rest", align = "center"})
hud.on("heal", function(id, e)
  if e.button == "left" then actions.heal() end
end)
```

Notes:
- A click reaches the element where it was drawn last frame. Hidden elements (`visible = false`) take no clicks.
- When elements overlap, the click goes to the topmost one that has a handler: a text drawn over a rectangle with a
  handler does not stop the rectangle's clicks.
- The handler runs as the player's input, so with the `actions` permission it may do game actions (`actions.heal()`
  above needs it).

## `hud.screen`

```lua
hud.screen() -> table
```

The game window's size in pixels, and how many pixels one UI unit takes. Use it to place elements against the window's
edges or centre, and to size them in step with the UI.

**Returns** `table`:

| Field | Type | Description |
| --- | --- | --- |
| `width` | `integer` | The game window's width in pixels. |
| `height` | `integer` | The game window's height in pixels. |
| `ui_scale` | `number` | Pixels per UI unit: multiply a size in UI units by it to get window pixels. |

**Example**

```lua
local s = hud.screen()
hud.text("centre", {x = s.width / 2, y = s.height / 2, text = "+", align = "center", size = 2})
```

## `hud.bounds`

```lua
hud.bounds(id) -> table?
```

Where one of your HUD elements was drawn last frame, in window pixels. Use it to place things against a text whose
width you do not know in advance.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The element's id. |

**Returns** `table` or `nil`: the rectangle the element covered, with integer fields `x`, `y` (its top left), `w` and
`h`. For a text with a `bg` box, it is the box. `nil` when the element was not drawn last frame: it does not exist, it
was only just created, it is hidden, or the interface is hidden.

**Example**

```lua
commands.register("measure", function()
  hud.text("title", {x = 20, y = 60, text = "A long title", size = 2})
  sleep_frames(2)
  local b = hud.bounds("title")
  if b then hud.rect("underline", {x = b.x, y = b.y + b.h, w = b.w, h = 2, colour = {255, 255, 255}}) end
end)
```
