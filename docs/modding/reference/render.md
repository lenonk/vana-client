# `render`

`render` changes how the world is drawn: how a single entity looks (hidden, renamed, recoloured, faded, tinted,
resized), extra post-process shader passes over the 3D scene, and the camera's distance and speed. No permission is
needed. Entity looks and shader passes belong to your mod: they take effect from the next drawn frame and are removed
when the mod unloads. Camera changes are queued (applied after the mod phase, so a value read back in the same frame
is the old one) and last until logout, even after the mod unloads. Scene-wide switches such as the wireframe view, the
background colour, the ambient light and the animation speed are behaviour switches: see
[`client.set_option`](client.md#clientset_option).

```lua
-- Hide every other player's model and name plate while the mod runs.
for _, e in ipairs(game.entities({ kinds = { "player" } })) do
  if not e.me then
    render.entity(e.id, { hidden = true })
  end
end
```

## Functions

| Function | What it does |
| --- | --- |
| [`render.entity(id, look?)`](#renderentity) | Sets or clears how one entity is drawn. |
| [`render.clear()`](#renderclear) | Clears every entity look the mod set. |
| [`render.shader(id, wgsl?)`](#rendershader) | Adds, replaces or removes a post-process shader pass. |
| [`render.shader_params(id, params)`](#rendershader_params) | Sets a shader pass's four parameter vectors. |
| [`render.camera_distance()`](#rendercamera_distance) | The camera distance scale a mod set. |
| [`render.set_camera_distance(scale)`](#renderset_camera_distance) | Sets the camera distance scale. |
| [`render.camera()`](#rendercamera) | The camera's settings and where it is. |
| [`render.set_camera(camera)`](#renderset_camera) | Changes the camera's distance, limit, speed and battle distance. |

## `render.entity`

```lua
render.entity(id, look?) -> nothing
```

Sets how one entity (a player, NPC, monster, or your own character) is drawn while your mod is loaded, or clears your
mod's look for it when `look` is `nil`. Each call replaces the entity's whole look: fields you leave out go back to
how the game draws it.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The entity's server id (`entity.id`; `game.player().id` for yourself). |
| `look` | `table` | no, default `nil` | The look (below); `nil` removes your mod's look for this entity. |

**`look` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `hidden` | `boolean` | `false` | `true` hides the entity's model and its name plate. |
| `name` | `string` | the entity's name | The name its plate shows. |
| `name_colour` | `{r, g, b, a}` | the game's colour | The plate's colour, 0..255 per channel (named or positional; each defaults to 255). |
| `alpha` | `number` | unchanged | Opacity 0..1 (clamped). The model is never drawn more opaque than the game would draw it. |
| `tint` | `{r, g, b}` | unchanged | Multipliers for the model's colour, 1 = unchanged, e.g. `{1, 0.5, 0.5}` or `{g = 0.5, b = 0.5}` for reddish (named or positional; each defaults to 1). |
| `scale` | `number` | unchanged | The model's size factor, clamped to 0.05..20. |

**Returns** nothing.

**Example**

```lua
-- Our target in a pale blue, half see-through, half size, renamed.
local t = game.target()
if t then
  render.entity(t.id, {
    name = "Practice Dummy", name_colour = { 120, 200, 255 },
    alpha = 0.5, tint = { 0.6, 0.8, 1.0 }, scale = 0.5,
  })
end
```

A look is kept by server id, so it applies again whenever that entity is drawn, until you clear it. When two mods set
a look for the same entity, the latest call wins; `render.entity(id, nil)` only removes a look your own mod set, and
the other mod's earlier look does not come back.

## `render.clear`

```lua
render.clear() -> nothing
```

Removes every entity look your mod set with `render.entity`. Other mods' looks stay.

**Returns** nothing.

**Example**

```lua
commands.register("looks_off", function()
  render.clear()
  print("Every entity looks as the game draws it again.")
end)
```

## `render.shader`

```lua
render.shader(id, wgsl?) -> nothing
```

Adds a post-process pass written in WGSL (the WebGPU shading language), replaces the source of your pass `id`, or
removes it when `wgsl` is `nil`. A pass runs once a frame over the finished 3D scene (including effects and `draw`
lines, rings and boxes), before the screen fades, name plates and the interface are drawn, so it changes the world
but not the menus.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The pass's name (your mod's own; another mod's `"blur"` is a different pass). |
| `wgsl` | `string` | no, default `nil` | The shader source (below); `nil` removes the pass. Removing a pass that does not exist does nothing. |

**Returns** nothing.

**Example**

```lua
-- A grayscale pass; its strength is post.p[0].x (0 = full colour, 1 = grey).
render.shader("gray", [[
@fragment fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  let c = textureSample(scene, samp, in.uv);
  let g = dot(c.rgb, vec3<f32>(0.299, 0.587, 0.114));
  return vec4<f32>(mix(c.rgb, vec3<f32>(g), post.p[0].x), c.a);
}
]])
render.shader_params("gray", { { 1 } })
```

**What you write.** Your source must define exactly this entry point; the client adds the declarations below in
front of it, so do not declare them yourself (and do not reuse the names `Post`, `VsOut`, `vs_main`, `scene`, `samp`,
`post` or `depth`):

```wgsl
@fragment fn fs_main(in: VsOut) -> @location(0) vec4<f32>
```

It is called once for every pixel of the scene. The colour it returns replaces that pixel (no blending), so return
the scene's sample for pixels you want unchanged.

**What the shader gets**

| Name | WGSL type | What it is |
| --- | --- | --- |
| `scene` | `texture_2d<f32>` | A copy of the frame drawn so far, including the passes before this one. Read it with `textureSample(scene, samp, uv)`. |
| `samp` | `sampler` | A linear-filtering sampler for `scene`. |
| `depth` | `texture_depth_2d` | The scene's depth buffer: 0.0 at the camera's near plane (0.1 yalms), 1.0 at the far plane (2000 yalms), not linear in distance. Read it with `textureLoad(depth, vec2<i32>(in.pos.xy), 0)`. `draw` shapes do not write into it. |
| `post.size` | `vec2<f32>` | The scene's size in pixels (width, height): the window's size, unless the 3D scene is drawn at another resolution. `1.0 / post.size` is one pixel in `uv` units. |
| `post.time` | `f32` | Seconds since this login, rising steadily; use it for animation. |
| `post.p[0]` .. `post.p[3]` | `vec4<f32>` each | Your four parameter vectors, set with [`render.shader_params`](#rendershader_params); all zeros until set. |
| `in.uv` | `vec2<f32>` | This pixel's place on the screen, 0..1: (0, 0) is the top-left corner, (1, 1) the bottom-right. |
| `in.pos` | `vec4<f32>` | This pixel's position in the scene, in pixels (`in.pos.xy`; pixel centres are at .5), the same units as `post.size`. |

The view distance in yalms of a pixel, from its depth `d`, is `0.1 * 2000.0 / (2000.0 - d * 1999.9)`. A complete pass
that adds distance fog with it:

```wgsl
@fragment fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  let c = textureSample(scene, samp, in.uv);
  let d = textureLoad(depth, vec2<i32>(in.pos.xy), 0);
  let dist = 0.1 * 2000.0 / (2000.0 - d * 1999.9);
  // post.p[0].x: the distance of full fog; post.p[1].rgb: the fog colour.
  let k = clamp(dist / max(post.p[0].x, 1.0), 0.0, 1.0);
  return vec4<f32>(mix(c.rgb, post.p[1].rgb, k), c.a);
}
```

Passes run in the order they were first added, across all mods; giving an existing pass a new source keeps its place
and its parameters. The source is compiled when the next frame is drawn: an error in it shows as a line in the chat log
(`render.shader <id>: <error>`) and the pass is skipped until you give it a source that compiles. Your passes are
removed when the mod unloads.

## `render.shader_params`

```lua
render.shader_params(id, params) -> nothing
```

Sets the four parameter vectors your pass reads as `post.p[0]` .. `post.p[3]`. Use it to drive a pass from Lua
(strength, colour, distances) without recompiling it. It takes effect from the next drawn frame.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | Your pass's name, as given to `render.shader`. |
| `params` | `number[][]` | yes | Up to four lists of up to four numbers: `params[1]` is `post.p[0]` (x, y, z, w), `params[2]` is `post.p[1]`, and so on. Give the numbers positionally, `{ {1, 0, 0, 0} }`, or by name, `{ {x = 1} }`. |

**Returns** nothing.

**Errors** `no shader <id>` when your mod has no pass with that id.

**Example**

```lua
-- Fade the "gray" pass in over two seconds with /grayin.
commands.register("grayin", function()
  for i = 1, 20 do
    render.shader_params("gray", { { i / 20 } })
    sleep(0.1)
  end
end)
```

Only the numbers you give change: `{ {0.5} }` sets `post.p[0].x` and leaves the other fifteen as they were. Extra
vectors or numbers past four are ignored.

## `render.camera_distance`

```lua
render.camera_distance() -> number
```

The camera distance scale a mod set with `render.set_camera_distance` or `render.set_camera`: how far the drawn eye is
from the point it looks at, as a share of the game's normal distance.

**Returns** `number`: the scale (for example 1.5). It is `0` until a mod has set a distance in this session;
`render.camera().distance` reports `1` in that case.

**Errors** `game state is not available here ...` in the lobby or inside a chat filter.

**Example**

```lua
local k = render.camera_distance()
print(("Camera distance: %.2f"):format(k > 0 and k or 1))
```

## `render.set_camera_distance`

```lua
render.set_camera_distance(scale) -> nothing
```

Sets how far the drawn eye is from the point it looks at, as a share of the game's normal distance (1 = normal, 2 =
twice as far). The camera still moves and swings as usual; only the eye's distance is scaled, and the eye is pulled in
short of walls behind it.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `scale` | `number` | yes | The distance scale, clamped to 0.5 .. `max_distance` (3 unless changed with `render.set_camera`, at most 10). |

**Returns** nothing.

**Example**

```lua
-- /far zooms out to twice the normal distance.
commands.register("far", function()
  render.set_camera_distance(2)
end)
```

Queued: applied after the mod phase. It is ignored while the camera is in first person, has no effect while locked on
to a target, and lasts until logout (also after the mod unloads). The same as `render.set_camera({distance = scale})`.

## `render.camera`

```lua
render.camera() -> table
```

The camera's settings and where it is now, as drawn.

**Returns** `table`:

| Field | Type | Description |
| --- | --- | --- |
| `distance` | `number` | The drawn eye's distance scale (1 unless a mod set it). |
| `max_distance` | `number` | The largest `distance` allowed (3 unless a mod changed it). |
| `speed` | `number` | The scale of how fast the camera keys turn and raise the camera (1 unless a mod set it). |
| `battle_distance` | `number?` | The distance used while engaged; `nil` when none is set. |
| `first_person` | `boolean` | Whether the camera is in first person. |
| `eye` | `{x, y, z}` | The camera's position, in world coordinates. |
| `look` | `{x, y, z}` | The point it looks at, in world coordinates. |

**Errors** `game state is not available here ...` in the lobby or inside a chat filter.

**Example**

```lua
local c = render.camera()
local dx, dz = c.look.x - c.eye.x, c.look.z - c.eye.z
print(("Camera %.1f yalms behind, distance x%.2f"):format(math.sqrt(dx * dx + dz * dz), c.distance))
```

## `render.set_camera`

```lua
render.set_camera(camera) -> nothing
```

Changes the camera's settings; only the fields you give change. Use it to let the player zoom out further, slow the
camera down, or pull out automatically in battle.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `camera` | `table` | yes | The settings to change (below). |

**`camera` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `distance` | `number` | unchanged | The drawn eye's distance scale, as `render.set_camera_distance`: clamped to 0.5 .. `max_distance`. |
| `max_distance` | `number` | unchanged | The largest distance scale allowed, clamped to 0.5..10 (3 to start with). A current distance above it is pulled in to it. |
| `speed` | `number` | unchanged | How fast the camera keys turn and raise the camera, as a scale: clamped to 0.1..5 (1 to start with). |
| `battle_distance` | `number` or `false` | unchanged | The distance scale while engaged (clamped to 0.5..10, and to `max_distance` when used): the camera moves to it on engaging and back to the earlier distance on disengaging. `false` removes it. |

**Returns** nothing.

**Errors** A conversion error when a field has the wrong type (for example `battle_distance = true`).

**Example**

```lua
-- Allow zooming out to 5x, pull out to 4x in battle, and turn a little slower.
render.set_camera({ max_distance = 5, battle_distance = 4, speed = 0.8 })
```

Queued: applied after the mod phase, limits first, so a `distance` given in the same call is clamped to the new
`max_distance`. Setting or removing `battle_distance` while engaged moves the camera at once. The settings last until
logout, also after the mod unloads.
