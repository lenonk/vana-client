# `ui.skin`

The `ui.skin` namespace changes how the modern UI looks: its sprites (bars, icons, job icons, buttons, list rows,
markers ...), its palette colours and its window frames. Every built-in panel draws with them, so a skin changes the
whole interface at once, and your mod's own windows (which use the same kit) change with it. Changes last while your
mod is loaded; when it unloads, the interface looks as it did. If two mods change the same thing, the last call wins,
and a mod giving a thing back only takes away its own change.

Every function that changes something needs the `ui.native` permission. To move, scale, fade or hide the built-in
panels instead, see [ui.native](ui.native.md#modern-panels).

The classic UI draws the game's own art from its DAT files: change it with a DAT overlay mod (a folder of replaced
files, see the Mods page), not with `ui.skin`.

```lua
-- Red HP bars, a warmer gold, and a darker window.
ui.skin.sprite("gauges/hp_full", "art/hp_full.png")
ui.skin.colour("gold", {255, 190, 90})
ui.skin.frame("window_normal", {top = {20, 16, 14, 235}, bottom = {8, 6, 6, 235}})
```

## Functions

| Function | What it does |
| --- | --- |
| [`ui.skin.sprite(name, path)`](#uiskinsprite) | Replaces a sprite with a PNG from your mod's folder. |
| [`ui.skin.colour(name, colour)`](#uiskincolour) | Sets a palette colour. |
| [`ui.skin.frame(name, frame)`](#uiskinframe) | Changes a window frame's border and its inside's colours. |
| [`ui.skin.list()`](#uiskinlist) | The names of every sprite, colour and frame you can change. |
| [`ui.skin.reset()`](#uiskinreset) | Gives back everything your mod changed. |

## `ui.skin.sprite`

```lua
ui.skin.sprite(name, path)
```

Replaces one of the modern UI's sprites with a PNG from your mod's folder. The sprite keeps its size on screen and its
nine-slice insets (the borders of a sprite that stretches, such as a bar or a row), so the PNG only has to be drawn at
the sprite's proportions: `ui.skin.list().sprites[name]` gives its size in logical pixels, and the pack's own art is
twice that. A bar is drawn as two sprites, `gauges/<kind>_empty` and `gauges/<kind>_full` (the full one cut at the
bar's value).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | yes | The sprite's name, such as `"gauges/hp_full"`, `"jobs/war"` or `"controls/list_row_selected"`. |
| `path` | `string` or `false` | yes | A PNG in your mod's folder; `false` gives the sprite back. |

**Returns** nothing.

**Needs** the `ui.native` permission.

**Errors**
- An unknown name: `no sprite "..." (ui.skin.list() names them)`.
- A file outside your mod's folder, missing, or not a PNG: the file's error.

**Example**

```lua
-- Your own job icons.
for _, job in ipairs({"war", "mnk", "whm", "blm", "rdm", "thf"}) do
  ui.skin.sprite("jobs/" .. job, "jobs/" .. job .. ".png")
end
```

## `ui.skin.colour`

```lua
ui.skin.colour(name, colour)
```

Sets one of the modern UI's palette colours. Text, titles, bars' numbers, cursors and the kit's widgets take their
colours from the palette: `"text"` (most text), `"muted"` (quiet text), `"gold"` (titles), `"focus"` (the keyboard
cursor's colour), `"hp"`, `"mp"`, `"tp"` and the rest that `ui.skin.list().colours` names.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | yes | The colour's name. |
| `colour` | `{r, g, b, a}` or `false` | yes | 0..255 each, `a` defaults to 255; `false` gives the colour back. |

**Returns** nothing.

**Needs** the `ui.native` permission.

**Errors** An unknown name: `no colour "..." (ui.skin.list() names them)`.

**Example**

```lua
ui.skin.colour("text", {235, 235, 235})
ui.skin.colour("gold", {140, 200, 255})
```

## `ui.skin.frame`

```lua
ui.skin.frame(name, frame)
```

Changes one of the modern UI's window frames: `"window_normal"` (most panels), `"window_focused"` (the panel with the
keys), `"window_inactive"`, `"window_alert"` (the locked-on target) and `"window_tooltip"`. A frame is a border drawn
over a vertical gradient. Give a new border, new colours for the gradient's top and bottom, or both; what you leave
out stays the pack's.

The border PNG is laid out as the pack's: a square that stands for the frame's `size` (`ui.skin.list().frames`), whose
corners (`corner` logical pixels square) are drawn as they are, and whose middle strips between them are tiled along
the edges. Leave its inside transparent: the gradient shows through it. Frames apply in the pack's border style
(Settings > Interface); the other border styles draw their own lines.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | yes | The frame's name. |
| `frame` | `table` or `false` | yes | `{path, top, bottom}`; `false` gives the frame back. |

**`frame` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `path` | `string` | the pack's border | A PNG in your mod's folder. |
| `top` | `{r, g, b, a}` | the pack's | The inside's colour at the top. |
| `bottom` | `{r, g, b, a}` | the pack's | The inside's colour at the bottom. |

The window opacity setting still applies over these colours.

**Returns** nothing.

**Needs** the `ui.native` permission.

**Errors** An unknown name: `no frame "..." (ui.skin.list() names them)`.

**Example**

```lua
ui.skin.frame("window_normal", {path = "frames/plain.png", top = {30, 30, 36, 240}, bottom = {12, 12, 16, 240}})
ui.skin.frame("window_focused", {path = "frames/plain_lit.png"})
```

## `ui.skin.list`

```lua
ui.skin.list() -> table
```

What can be changed.

**Returns** `table`, never nil:

| Field | Type | Description |
| --- | --- | --- |
| `sprites` | `table<string, {w, h}>` | Every sprite by name, with its size in logical pixels. |
| `colours` | `string[]` | The palette's colour names. |
| `frames` | `table<string, {size, corner}>` | Every window frame by name, with its size and corner in logical pixels. |

**Example**

```lua
for name, s in pairs(ui.skin.list().sprites) do
  if name:find("^gauges/") then print(name, s.w, s.h) end
end
```

## `ui.skin.reset`

```lua
ui.skin.reset()
```

Gives back every sprite, colour and frame your mod changed (other mods' changes stay). Unloading your mod does the
same.

**Returns** nothing.
