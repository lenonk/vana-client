# `hud`

The `hud` namespace draws things over the game: texts, filled rectangles, speech bubbles, bars, a map, PNG images, live
portraits of actors and item, status, spell or ability icons. Each element has an id you choose. You create it once (`hud.text`, `hud.rect`, ...), change it with `hud.set`
as things happen, and it stays on screen until you remove it; you do not redraw it every frame. HUD elements draw over
the game world and the name plates and under every window, in the game world only, and they hide while the player has
the interface hidden. Each interface draws them as it draws its built-in ones: the classic UI with the game's art and fonts, the
modern UI with its painter (its faces, smooth shapes, the pack's gauges), so an element looks as a built-in one
does. None of these functions needs a permission. Clicks, drags and the pointer coming and going on an
element can be handled with `hud.on`, and any element can carry a tooltip.

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
| [`hud.bubble(id, spec)`](#hudbubble) | Creates or replaces a speech bubble with a tail pointing at a place. |
| [`hud.bar(id, spec)`](#hudbar) | Creates or replaces a bar filled from 0 to 1. |
| [`hud.map(id, spec)`](#hudmap) | Creates or replaces a window onto the zone's map. |
| [`hud.image(id, spec)`](#hudimage) | Creates or replaces a PNG image from the mod's folder. |
| [`hud.portrait(id, spec)`](#hudportrait) | Creates or replaces a live portrait of an actor's face. |
| [`hud.icon(id, spec)`](#hudicon) | Creates or replaces an item's, a status effect's, a spell's or an ability's icon. |
| [`hud.set(id, fields)`](#hudset) | Changes some fields of an element. |
| [`hud.remove(id)`](#hudremove) | Removes an element. |
| [`hud.clear()`](#hudclear) | Removes every element of the mod. |
| [`hud.on(id, handler)`](#hudon) | Sets the function called when the player clicks or drags an element, or the pointer comes or goes. |
| [`hud.off(id)`](#hudoff) | Removes an element's handler. |
| [`hud.opacity(window, opacity)`](#hudopacity) | Sets the opacity of one of your HUD windows. |
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
| `anchor` | `string` | none | A modern [panel](ui.native.md#modern-panels)'s name: `x` and `y` become offsets from one of the panel's corners (`corner`), negative ones going left or up, so the element follows the panel. A hidden panel keeps its last place; an element anchored to a panel never drawn yet is not drawn either. HUD elements draw under the panels, so put them beside a panel, or where a hidden one was. `false` in `hud.set` goes back to the window. Not in the classic UI. |
| `corner` | `string` | `"top_left"` | With `anchor`: the panel's corner the offsets count from, `"top_left"`, `"top_right"`, `"bottom_left"` or `"bottom_right"`. `{anchor = "party", y = -24}` stands just above the party window. |
| `window` | `string` | `"hud"` | The HUD window the element belongs to. A HUD window is a group of your elements the client treats as one window: [`hud.opacity`](#hudopacity) sets its opacity, over everything its elements draw. |
| `tip` | `string` or `table` | none | A tooltip shown while the pointer rests on the element: a string, or `{title = ..., text = ...}` (the title above the text; `\n` breaks lines). `false` in `hud.set` removes it. The modern UI shows it in its tooltip frame, the classic UI in a dark box by the pointer. |

**Create and set.** Each create call (`hud.text`, `hud.rect`, ...) makes a new element from its spec: fields you leave
out take their defaults. If your mod already has an element with that id, of any kind, it is replaced, keeping its
place in the drawing order. `hud.set` changes only the fields you give. Changes apply at once (they show from the next
frame drawn).

**Colours** are `{r, g, b, a}` tables with values 0 to 255, by position (`{255, 0, 0}`) or by name
(`{r = 255, g = 0, b = 0}`); a missing channel is 255. `false` means no colour (it clears one in `hud.set`).
Anything else raises `a colour is {r, g, b, a} (0..255), or false for none`.

**Clicks.** An element takes clicks and drags only once you give it a handler with [`hud.on`](#hudon). One with a
handler or a `tip` reports the pointer coming and going.

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

**Errors** A colour field that is not a table or `false`: `a colour is {r, g, b, a} (0..255), or false for none`. A field of the wrong kind raises a
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
| `line` | `number` | none | Draws only its outline, this many window pixels wide (0.5 to 32), inside the rectangle, instead of filling it. `false` in `hud.set` fills it again. |
| `round` | `number` | `0` | Rounds its corners this many window pixels (the outer edge's radius), as the modern UI's frames are. The classic UI draws square corners. |

**Returns** nothing.

**Errors** A `colour` that is not a table or `false`: `a colour is {r, g, b, a} (0..255), or false for none`.

**Example**

```lua
hud.rect("flash", {x = 0, y = 0, w = hud.screen().width, h = 6, colour = {255, 40, 40, 200}})
timer.after(0.5, function() hud.remove("flash") end)
```

## `hud.bubble`

```lua
hud.bubble(id, spec)
```

Creates or replaces a speech bubble: a rectangle with rounded corners, an optional outline, and a short tail that
points at a place on screen, such as a speaker's head. The tail leaves the side of the bubble that faces the point,
as near the point as the corners allow, and is never longer than `tail_len` (it stops short of a near point, and leans
out at most 60 degrees from its side). There is no tail while the point is inside the bubble. The outline goes round
the tail too, and the edges are smoothed. Both interfaces draw the same shape. The bubble holds nothing itself: put
texts, a portrait or icons over it on a higher `layer`.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The element's id. |
| `spec` | `table` | yes | The bubble's fields. |

**`spec` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `x`, `y`, `visible`, `layer` | | | As for [every element](#places-sizes-and-layers). |
| `w`, `h` | `number` | `200`, `60` | Its size in window pixels, without the tail. |
| `round` | `number` | `10` | The corners' radius in window pixels. |
| `colour` | `{r, g, b, a}` | `{14, 16, 26, 235}` | The fill. |
| `edge` | `{r, g, b, a}` or `false` | none | The outline's colour. |
| `edge_w` | `number` | `2` | The outline's width in window pixels. |
| `tail` | `{x, y}` or `false` | none | The point the tail aims at, in window pixels (as `draw.to_screen` gives). |
| `tail_w` | `number` | `18` | The tail's width where it leaves the bubble. |
| `tail_len` | `number` | `22` | The tail's longest length. |

**Returns** nothing.

**Errors**
- A `tail` that is not a point or `false`: `tail is a point {x, y}, or false for none`.
- A `colour` or `edge` that is not a table or `false`: `a colour is {r, g, b, a} (0..255), or false for none`.

**Example**

```lua
-- a bubble over our target's head, its tail at the head
events.on("frame", function()
  local t = game.target()
  local head = t and t.pos and draw.to_screen({x = t.pos.x, y = t.pos.y - (t.height or 1.8), z = t.pos.z})
  if not (head and head.on_screen) then
    hud.remove("say")
    hud.remove("say_text")
    return
  end
  hud.bubble("say", {x = head.x - 90, y = head.y - 90, w = 180, h = 50, round = 12, edge = {196, 160, 96},
    tail = {x = head.x, y = head.y}})
  hud.text("say_text", {x = head.x, y = head.y - 76, text = "Hello!", align = "center", layer = 1})
end)
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
| `colour` | `{r, g, b, a}` | none | A fill colour you choose, used instead of `kind`'s. |

**Returns** nothing.

**Errors** A `colour` that is not a table or `false`: `a colour is {r, g, b, a} (0..255), or false for none`.

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

Creates or replaces a window onto the map of the place the player is in, showing what the map window shows there
(see `game.map`): the map; the game's NO MAP image over the map's place while the map is not yours (or not known to be
yet), the marker still where you are; or a record's image where the place needs no map, without a marker. By
default it is centred on the player and follows them. Where no map exists, nothing is drawn.

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
| `alpha` | `number` | the client option `map_opacity` | The map image's opacity, 0 to 1 (the marker stays solid). Left out (or `false` in `hud.set`), the client applies its `map_opacity` switch: the player's Map Overlay Opacity, a setting of the Map Overlay mod. |
| `me` | `boolean` | `false` | Draws your marker at your position on the map, facing your way, as the game's map windows draw it in each interface (the classic map arrow, the modern UI's orange arrow). |

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
- A `colour` that is not a table or `false`: `a colour is {r, g, b, a} (0..255), or false for none`.

**Example**

```lua
hud.image("logo", {x = -74, y = -74, path = "img/logo.png", w = 64, h = 64, colour = {255, 255, 255, 200}})
```

`hud.set(id, {path = "img/other.png"})` swaps the picture; it takes the new file's size unless the same call gives `w` and `h`. A file changed on disk shows the next time it is loaded (`hud.image` or a `path` in `hud.set`).

## `hud.portrait`

```lua
hud.portrait(id, spec)
```

Creates or replaces a portrait: the actor's head and shoulders drawn live every frame from its model, from straight
in front of its face, lit as the scene around it. It moves as the actor does (talking, turning its head, a changed
look). An oval by default, or square; a backing and a frame ring are drawn into it, both following its shape. It shows
only while the actor is drawn in the world: nothing while it is out of view, too far, hidden or not loaded, and nothing
at all before its model is ready. Up to four actors have portraits at once; an actor shown by several elements is
drawn once, as the first of them (by layer) asks.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The element's id. |
| `spec` | `table` | yes | The portrait's fields. |

**`spec` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `x`, `y`, `visible`, `layer` | | | As for [every element](#places-sizes-and-layers). |
| `entity` | `integer` | (required) | The actor's server id, as `game.entity` and `game.target` give it. |
| `size` | `number` | | Sets `w` and `h` at once. |
| `w`, `h` | `number` | `96`, `96` | Its width and height in window pixels; the view widens or narrows with them. |
| `shape` | `string` | `"oval"` | `"oval"` (the oval filling `w` by `h`) or `"square"`. |
| `bg` | `{r, g, b, a}` or `false` | none (clear) | The backing behind the actor, in its shape. |
| `frame` | `{r, g, b, a}` or `false` | none | A frame ring round its edge, lit from above. |
| `frame_w` | `number` | `3` | The frame's width in window pixels. |
| `colour` | `{r, g, b, a}` | white | A tint its colours are multiplied by; `a` fades it. |

**Returns** nothing.

**Errors**
- No `entity`: `a portrait needs entity (a server id)`.
- A `shape` other than those two: `shape "...": oval or square`.
- A `colour`, `bg` or `frame` that is not a table or `false`: `a colour is {r, g, b, a} (0..255), or false for none`.

**Example**

```lua
-- the current target's face in the top right corner
events.on("target_change", function(id)
  if id then
    hud.portrait("face", {x = -120, y = 20, entity = id, w = 96, h = 120, bg = {14, 16, 26, 220},
      frame = {196, 160, 96}})
  else
    hud.remove("face")
  end
end)
```

`hud.set(id, {entity = other})` moves the portrait to another actor.

## `hud.icon`

```lua
hud.icon(id, spec)
```

Creates or replaces a square icon by id: an item's, a status effect's, or a spell's or an ability's (weapon skills
and pet commands too) as the magic and ability lists draw them.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The element's id. |
| `spec` | `table` | yes | The icon's fields. |

**`spec` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `x`, `y`, `visible`, `layer` | | | As for [every element](#places-sizes-and-layers). |
| `kind` | `string` | item | `"item"`, `"status"`, `"spell"` or `"ability"` (`"weaponskill"` is the same as `"ability"`). Any other value, or none, is an item's icon. List and canvas icons take the same kinds. |
| `id` | `integer` | `0` | The item id, the status effect id, the spell id (as `res.spell`), or the ability id (as `res.ability`: weapon skills 0..0x1ff, job abilities from 0x200, pet commands from 0x400). |
| `size` | `number` | `32` | Its width and height in window pixels (at least 1). |

**Returns** nothing.

**Example**

```lua
local x = 20
for _, b in ipairs(game.buffs()) do
  hud.icon("buff" .. b.id, {x = x, y = 120, kind = "status", id = b.id, size = 24})
  x = x + 26
end
-- Cure (spell 1) and Provoke (ability 0x200 + 35).
hud.icon("cure", {x = 20, y = 160, kind = "spell", id = 1, size = 32})
hud.icon("provoke", {x = 56, y = 160, kind = "ability", id = 0x200 + 35, size = 32})
```

## `hud.set`

```lua
hud.set(id, fields)
```

Changes some fields of one of your mod's HUD elements; the others keep their values. The fields are those of the
element's create call (`hud.text`, `hud.bar`, ...); fields that belong to other kinds are ignored. An element's kind
cannot change: create it again for that. A field left out keeps its value, so to take away a text's `colour`, `bg` or
`outline`, or a bar's `colour`, set it to `false`. An image takes a new `path`.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The element's id. |
| `fields` | `table` | yes | The fields to change. |

**Returns** nothing.

**Errors** No element with that id: `no HUD element "..."`. Fields of the wrong kind raise the same errors as the
create call. A `path` on an element that is not an image: `path is an image's field`; a `path` that cannot be loaded
raises the errors of [`hud.image`](#hudimage).

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

Removes one of your mod's HUD elements, and its click handler (`hud.on`). An id with no element is ignored.

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

Removes every HUD element of your mod and every click handler set with `hud.on` (other mods' elements stay).

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

Sets the function called when the player clicks or drags one of your HUD elements with the left or the right mouse
button, and when the pointer comes onto it or leaves it. A press on the element belongs to your mod: the game sees
neither it nor its release, and `input.on_mouse` filters see both with `blocked = true`. An element has one handler: a
second call replaces the first. You may set it before the element exists. `hud.off`, `hud.remove` and `hud.clear` take
it away.

- **Click:** `"click"` comes when the button is released, if the pointer moved less than 4 pixels since the press.
- **Drag:** a press that moves 4 pixels or more is a drag. The handler gets `"drag_start"` (at the place the button
  went down), `"drag"` as the pointer moves (at most once a frame), and `"drag_end"` at the release, wherever the
  pointer is by then. `dx` and `dy` are the pointer's offset from where the button went down. `drag_end` names your
  topmost other element with a handler under the pointer in `over` (an icon or a label over a slot is passed by), and
  that one gets `"drop"` with the dragged element in `from`. That is enough to drag one slot onto another, or to move a bar: set its `x` and
  `y` from `dx` and `dy` while it drags.
- **Pointer:** `"mouse_enter"` and `"mouse_leave"` come as the pointer comes onto the element and leaves it (with
  `x` and `y` only). Elements with a `tip` show it automatically; use these for anything more (a slot showing its
  macro's lines in another element).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The element's id. |
| `handler` | `function` | yes | Called as `handler(id, event)`. |

`event` is a table:

| Field | Type | Description |
| --- | --- | --- |
| `kind` | `string` | `"click"`, `"drag_start"`, `"drag"`, `"drag_end"`, `"drop"`, `"mouse_enter"` or `"mouse_leave"`. |
| `id` | `string` | The element's id (for `"drop"`, the element dropped on). |
| `button` | `string` | `"left"` or `"right"` (not on `"mouse_enter"` / `"mouse_leave"`). |
| `x`, `y` | `integer` | The pointer, in window pixels. |
| `dx`, `dy` | `integer` | The pointer's offset from where the button went down (0 for a click). |
| `over` | `string` or `nil` | `"drag_end"` only: your topmost other element with a handler under the pointer at the release. |
| `from` | `string` | `"drop"` only: the element that was dragged. |

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

Dragging a bar to a new place, and swapping two slots by dropping one on the other:

```lua
local at = {x = 400, y = 600}
hud.bar("xp", {x = at.x, y = at.y, w = 300, h = 10, kind = "experience", tip = "Drag to move"})
hud.on("xp", function(id, e)
  if e.kind == "drag" or e.kind == "drag_end" then
    hud.set(id, {x = at.x + e.dx, y = at.y + e.dy})
    if e.kind == "drag_end" then at.x, at.y = at.x + e.dx, at.y + e.dy end
  end
end)

for i, spell in ipairs({1, 2}) do
  local id = "slot" .. i
  hud.icon(id, {x = 20 + 40 * (i - 1), y = 200, kind = "spell", id = spell, size = 32, tip = res.spell(spell).name})
  hud.on(id, function(id, e)
    if e.kind == "drop" then print(e.from .. " dropped on " .. id) end
  end)
end
```

Notes:
- A press reaches the element where it was drawn last frame. Hidden elements (`visible = false`) take no clicks.
- When elements overlap, the press goes to the topmost one that has a handler: a text drawn over a rectangle with a
  handler does not stop the rectangle's clicks.
- Clicks, drags and drops run as the player's input, so with the `actions` permission they may do game actions
  (`actions.heal()` above needs it). `mouse_enter` and `mouse_leave` do not.

## `hud.off`

```lua
hud.off(id)
```

Removes the handler of one of your HUD elements; the element stays and clicks go through to the game again. An
id without a handler is ignored.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The element's id. |

**Returns** nothing.

**Example**

```lua
-- The "heal" button above stops taking clicks while you are engaged.
events.on("status_change", function(status)
  if status == 1 then hud.off("heal") end
end)
```

## `hud.opacity`

```lua
hud.opacity(window, opacity)
```

Sets the opacity of one of your HUD windows: every element whose `window` field names it (`"hud"` for elements that
name none) draws at it, as the client draws every window, built in or a mod's, at its opacity. The player's
setting for the window, when there is one, wins over yours.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `window` | `string` | yes | The HUD window's name. |
| `opacity` | `number` | no | 0 (unseen) to 1 (opaque); `nil` gives it back (opaque). |

**Returns** nothing.

**Example**

```lua
hud.text("dps", {x = 20, y = 300, window = "meter", text = "DPS 0"})
hud.bar("dps_bar", {x = 20, y = 320, w = 160, h = 8, window = "meter"})
hud.opacity("meter", 0.7)
```

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
