# `draw`

`draw` puts shapes into the 3D world: lines, flat rings, wire boxes, text labels and pictures, all placed in world
coordinates (yalms) exactly as entity positions are, so you can pass an entity's `pos` straight in. Every shape has an
id you choose; calling a function again with the same id replaces that shape, and a shape stays until you remove it
with `draw.remove` or `draw.clear`, or the mod unloads. No permission is needed, and the calls work at any time. The
usual pattern is to update a shape from a `frame` handler or a timer as the thing it marks moves.

```lua
-- A red ring under the current target, kept up to date every frame.
events.on("frame", function()
  local t = game.target()
  if t then
    draw.ring("target", { at = t.pos, radius = 3, colour = { 255, 64, 64, 200 } })
  else
    draw.remove("target")
  end
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`draw.line(id, spec)`](#drawline) | Draws a straight line between two world points. |
| [`draw.ring(id, spec)`](#drawring) | Draws a flat circle, outlined or filled, around a world point. |
| [`draw.box(id, spec)`](#drawbox) | Draws the edges of an axis-aligned box. |
| [`draw.text(id, spec)`](#drawtext) | Draws a text label at a world point. |
| [`draw.image(id, spec)`](#drawimage) | Draws a PNG from the mod's folder on a flat rectangle in the world. |
| [`draw.remove(id)`](#drawremove) | Removes one of the mod's shapes. |
| [`draw.clear()`](#drawclear) | Removes all of the mod's shapes. |
| [`draw.to_screen(point)`](#drawto_screen) | Tells where a world point appears in the game window. |

**Points and colours.** A point is a table `{x = ..., y = ..., z = ...}` or `{x, y, z}` (positional); a missing
component counts as 0. x and z are the ground plane and y is the height, pointing down: a smaller y is higher, so to
go up from a position you subtract from its y. A colour is `{r = ..., g = ..., b = ..., a = ...}` or `{r, g, b, a}`,
each an integer 0..255 that defaults to 255 when left out (so `{255, 0, 0}` is opaque red); a value outside 0..255
raises a conversion error. A shape with no `colour` (or a `colour` that is not a table) is white.

**How shapes are drawn.** Lines, rings and boxes are drawn into the 3D scene: anything that stands in front of them
(walls, characters) hides them, and they are blended by their alpha. Texts and images are drawn with the interface,
over name plates and under windows; scenery in front of them hides them too, and they are not drawn while the
interface is hidden. Ids belong to your mod and are shared by every kind of shape: `draw.ring("a", ...)` replaces a
line called `"a"`, and another mod's `"a"` is a different shape. `/lua hide` hides every mod's shapes until
`/lua show`.

## `draw.line`

```lua
draw.line(id, spec) -> nothing
```

Creates a straight, thin line between two world points, or replaces the shape `id` with it. Use it for pointers,
paths and range markers.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The shape's name; reusing it replaces the shape. |
| `spec` | `table` | yes | Where and how to draw it (below). |

**`spec` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `from` | point | required | One end of the line, in world coordinates. |
| `to` | point | required | The other end. |
| `colour` | colour | white | The line's colour; `a` makes it see-through. |

**Returns** nothing.

**Errors** `a point is {x, y, z}` when `from` or `to` is missing or is not a table.

**Example**

```lua
-- A line from our feet to our target's feet.
local me, t = game.player(), game.target()
if t then
  draw.line("to_target", { from = me.pos, to = t.pos, colour = { 255, 255, 0 } })
end
```

## `draw.ring`

```lua
draw.ring(id, spec) -> nothing
```

Creates a flat circle around a world point, or replaces the shape `id` with it. The ring lies on the horizontal
plane at the height of its centre. Use it to mark a range (spell reach, an area of effect) or a target.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The shape's name; reusing it replaces the shape. |
| `spec` | `table` | yes | Where and how to draw it (below). |

**`spec` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `at` | point | required | The centre, in world coordinates. |
| `radius` | `number` | `1` | The radius in yalms (negative values count as 0). |
| `segments` | `integer` | `48` | How many straight pieces make up the circle, 3..360 (clamped). More is rounder. |
| `filled` | `boolean` | `false` | `false` draws the outline; `true` draws a filled disc. |
| `colour` | colour | white | The ring's colour; use a low `a` for a see-through filled disc. |

**Returns** nothing.

**Errors** `a point is {x, y, z}` when `at` is missing or is not a table.

**Example**

```lua
-- A see-through disc showing 20 yalms around us, with a solid edge.
local me = game.player()
draw.ring("reach_fill", { at = me.pos, radius = 20, filled = true, colour = { 80, 160, 255, 50 } })
draw.ring("reach_edge", { at = me.pos, radius = 20, colour = { 80, 160, 255 } })
```

Filled shapes are drawn before outlines, so an outline on the same spot always shows over a filled disc.

## `draw.box`

```lua
draw.box(id, spec) -> nothing
```

Creates the twelve edges of a box, or replaces the shape `id` with it. The box's sides are always lined up with the
world's x, y and z axes (it cannot be turned).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The shape's name; reusing it replaces the shape. |
| `spec` | `table` | yes | Where and how to draw it (below). |

**`spec` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `at` | point | required | The centre of the box, in world coordinates. |
| `size` | point | required | The box's full length along x, its height along y and its depth along z, in yalms. |
| `colour` | colour | white | The edges' colour. |

**Returns** nothing.

**Errors** `a point is {x, y, z}` when `at` or `size` is missing or is not a table.

**Example**

```lua
-- A 1 x 2 x 1 yalm box standing on the ground where we are.
-- y points down, so the centre is 1 yalm *above* our feet: pos.y - 1.
local p = game.player().pos
draw.box("pillar", { at = { x = p.x, y = p.y - 1, z = p.z }, size = { 1, 2, 1 }, colour = { 0, 255, 128 } })
```

## `draw.text`

```lua
draw.text(id, spec) -> nothing
```

Creates a text label at a world point, or replaces the shape `id` with it. The text is centred on the point and keeps
the same size on screen however far away the point is. Use it to label places or entities.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The shape's name; reusing it replaces the shape. |
| `spec` | `table` | yes | What to write and where (below). |

**`spec` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `at` | point | required | The point the text is centred on, in world coordinates. |
| `text` | `string` | `""` | The text to show. |
| `size` | `number` | `1` | The text's scale: 1 is the interface's normal text size; clamped to 0.25..8. |
| `colour` | colour | white | The text's colour. |
| `outline` | colour | none | When given, the text gets a one-pixel outline of this colour all around it. |

**Returns** nothing.

**Errors** `a point is {x, y, z}` when `at` is missing or is not a table; `a colour is {r, g, b, a} (0..255)` when
`outline` is given but is not a table.

**Example**

```lua
-- Our target's name, 2 yalms above its feet, in yellow with a black outline.
local t = game.target()
if t then
  local above = { x = t.pos.x, y = t.pos.y - 2, z = t.pos.z }
  draw.text("label", { at = above, text = t.name, colour = { 255, 230, 0 }, outline = { 0, 0, 0 } })
end
```

The label is not drawn while its point is behind the camera, and it is hidden behind scenery in front of it.

## `draw.image`

```lua
draw.image(id, spec) -> nothing
```

Creates a picture from a PNG file in your mod's folder, shown on a flat rectangle in the world, or replaces the shape
`id` with it. The rectangle can lie on the ground, stand upright, or always face the camera.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The shape's name; reusing it replaces the shape. |
| `spec` | `table` | yes | The picture and where to put it (below). |

**`spec` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `path` | `string` | required | The PNG file, relative to the mod's folder (e.g. `"img/arrow.png"`). |
| `at` | point | required | The centre of the rectangle, in world coordinates. |
| `w` | `number` | `2` | The rectangle's width in yalms (at least 0.01). |
| `h` | `number` | `2` | The rectangle's height in yalms (at least 0.01). |
| `facing` | `string` | `"ground"` | `"ground"`: lies flat, the top of the picture pointing toward `yaw`. `"upright"`: stands up, top upward, its front toward `yaw` (it reads correctly from that side). `"camera"`: always turned to face the camera, top upward. Any other value counts as `"ground"`. |
| `yaw` | `number` | `0` | The direction in radians, measured as entity headings are (0 is east). Not used with `"camera"`. |
| `colour` | colour | white | A tint the picture is multiplied by; white leaves it unchanged, and `a` fades it. |

**Returns** nothing.

**Errors**
- `<path>: outside the mod's folder` when `path` contains `..` or `:`, or starts with `/`.
- `<path>: <reason>` when the file cannot be read (for example, it does not exist).
- `<path>: <reason> (a PNG)` when the file is not a PNG the client can read.
- `bundled mods carry no image files` for a mod that ships inside the client.
- A conversion error when `path` is missing.
- `a point is {x, y, z}` when `at` is missing or is not a table.

**Example**

```lua
-- An arrow painted on the ground at our feet, pointing the way we face.
local me = game.player()
draw.image("arrow", {
  path = "arrow.png", at = me.pos, w = 1.5, h = 1.5,
  facing = "ground", yaw = me.heading,
})
```

The file is read from disk on every call, so for a moving picture prefer few calls (a timer) over one every frame.
Like texts, pictures are drawn with the interface: hidden by scenery in front of them and not drawn while the
interface is hidden.

## `draw.remove`

```lua
draw.remove(id) -> nothing
```

Removes one of your mod's shapes by id. Removing an id that does not exist does nothing.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The shape to remove. |

**Returns** nothing.

**Example**

```lua
events.on("target_change", function(id)
  if id == nil then
    draw.remove("target")
  end
end)
```

## `draw.clear`

```lua
draw.clear() -> nothing
```

Removes every shape your mod has drawn. Other mods' shapes stay.

**Returns** nothing.

**Example**

```lua
commands.register("marks_off", function()
  draw.clear()
  print("All markers removed.")
end)
```

## `draw.to_screen`

```lua
draw.to_screen(point) -> {x: integer, y: integer, on_screen: boolean}?
```

Tells where a world point appears in the game window, using the camera as it was last drawn. Use it to place HUD
elements (which take window pixels) next to something in the world.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `point` | point | yes | The world point, e.g. an entity's `pos`. |

**Returns** `table?`: `nil` when the point is behind the camera; otherwise a table:

| Field | Type | Description |
| --- | --- | --- |
| `x` | `integer` | Pixels from the window's left edge, as `hud` positions take them. |
| `y` | `integer` | Pixels from the window's top edge. |
| `on_screen` | `boolean` | `true` when the point is inside the window and within the camera's view distance. When `false`, `x` and `y` may lie outside the window. |

**Errors** `a point is {x, y, z}` when `point` is not a table.

**Example**

```lua
-- A HUD label that follows our target on screen.
events.on("frame", function()
  local t = game.target()
  local p = t and draw.to_screen(t.pos)
  if p and p.on_screen then
    hud.text("tag", { x = p.x, y = p.y - 40, text = t.name, align = "center" })
  else
    hud.remove("tag")
  end
end)
```
