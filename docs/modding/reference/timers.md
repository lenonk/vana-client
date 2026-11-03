# `timers`

Rows in the client's Timers window: a name, a countdown and a gauge, in one of the window's three sections (recasts,
buffs, debuffs), optionally under a group heading. A mod's rows show even when the Timers Extra is off. Each mod has
its own rows, known by name. The calls are queued and take effect after the mod phase; in the lobby they do nothing.
No permission is needed.

```lua
-- A row counting down each buff we gain, with its icon (60 s, for the example).
events.on("buff_gain", function(id)
  timers.add(res.status(id) or ("Status " .. id), 60, { icon = { kind = "status", id = id } })
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`timers.add(name, seconds, options?)`](#timersadd) | Adds or replaces one of the mod's rows, counting down `seconds`. |
| [`timers.remove(name)`](#timersremove) | Removes the mod's row by name. |
| [`timers.clear()`](#timersclear) | Removes every row of the mod's. |

## `timers.add`

```lua
timers.add(name, seconds, options?)
```

Adds a row that counts down from `seconds`, or replaces the mod's row of the same name (starting its count again).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | yes | The row's text, and its key: another `timers.add` with the same name replaces this row. |
| `seconds` | `number` | yes | Seconds from now until the row runs out. Below 0 counts as 0. |
| `options` | `table` | no | See below. |

**`options` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `section` | `string` | `"buff"` | Where the row goes: `"recast"` (or `"recasts"`), `"debuff"` (or `"debuffs"`); anything else is the buffs section. |
| `group` | `string` | none | A heading the row is listed under (for example a target's name). Without one, a recast row has no heading and a buff or debuff row goes under "Timers". |
| `icon` | `table` | none | `{kind = "status" \| "spell" \| "ability", id = number}`: a status effect's icon, or a spell's or an ability's icon as their menus show it. `kind` defaults to `"status"`; `id` is required. When nothing matches, the row has no icon. |
| `total` | `number` | `seconds` | The gauge's whole length in seconds. With a `total` longer than `seconds` the gauge starts part of the way. |
| `fills` | `boolean` | `true` for a recast row, else `false` | `true`: the gauge fills up toward ready, as recasts do; `false`: it drains. |

**Returns** nothing.

**Errors** when `icon` is given without a numeric `id`.

**Example**

```lua
-- //pop <name>: a 5-minute debuff-section row under the "Spawns" heading.
commands.register("pop", function(name)
  timers.add(name or "Pop", 300, { section = "debuff", group = "Spawns" })
end)
```

A row disappears one second after it reaches zero, or when the mod unloads. Within a group, rows are sorted by the
time left.

## `timers.remove`

```lua
timers.remove(name)
```

Removes the mod's row with this name. A name with no row does nothing.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | yes | The name given to `timers.add`. |

**Returns** nothing.

**Example**

```lua
events.on("buff_lose", function(id)
  timers.remove(res.status(id) or ("Status " .. id))
end)
```

## `timers.clear`

```lua
timers.clear()
```

Removes every row this mod added. Other mods' rows and the client's stay.

**Returns** nothing.

**Example**

```lua
events.on("zone_change", function()
  timers.clear()
end)
```
