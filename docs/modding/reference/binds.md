# `binds`

The `binds` namespace ties a key combination to a text line or a Lua function, as the game's key binds do. A bound key is taken from the game: when it fires, the game and its windows do not see that keystroke. Setting binds needs no permission, but a bound line other than a `//` mod command is a game action and needs the `actions` permission when the key fires (a bind is the player's input, so `actions` is enough; `automation` is not needed). Binds belong to the mod: they go away when it unloads, and they work only while the character is in the world. Keys are written as described in [Key combinations](input.md#key-combinations).

```lua
binds.set("f9", "//mymod toggle")        -- a mod command: no permission needed
binds.set("ctrl+f9", "/heal")            -- a game command: needs actions
binds.set("alt+1", function()
  print("Alt+1 pressed")
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`binds.set(key, action, options?)`](#bindsset) | Binds a key combination to a line or a function. |
| [`binds.unset(key)`](#bindsunset) | Removes the mod's binds of a key combination. |
| [`binds.list()`](#bindslist) | The mod's binds. |

## `binds.set`

```lua
binds.set(key, action, options?)
```

Binds `key` (with exactly the modifiers given) to `action`. When the key fires, a line runs as if the player had typed it, or a function is called.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `key` | `string` | yes | A [key combination](input.md#key-combinations): `"f9"`, `"ctrl+1"`, `"^!f1"` (Ctrl+Alt+F1), `"shift+numpad+"`. |
| `action` | `string` or `function` | yes | A line to run as typed (`"//mymod go"`, `"/heal"`), or a function called with no arguments. |
| `options` | `table` | no | When the bind fires; the fields are below. |

**`options` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `up` | `boolean` | `false` | Fire when the key is released instead of pressed. The press itself then still reaches the game; only the release is taken. |
| `typing` | `boolean` | `false` | Fire while a text field (the chat line, a search...) has the keyboard too. Without it the key goes to the text field then, and the bind does not fire. |

**Returns** nothing.

**Errors**
- `no key "<key>" (e.g. "f1", "ctrl+a", "^!1", "numpad+")` when `key` is not a known combination.
- `binds.set(key, line or function[, {up, typing}])` when `action` is neither a string nor a function.

**Example**

```lua
-- a toggle on F10 that fires when the key is released
local on = false
binds.set("f10", function()
  on = not on
  chat.add("helper " .. (on and "on" or "off"))
end, { up = true })
```

Notes:
- The modifiers must match exactly: `"f9"` fires only when no Ctrl, Alt, Shift or Win is held, and `"ctrl+f9"` only with Ctrl alone.
- Holding the key down does not fire the bind again: repeats are ignored (for a press bind they are still taken from the game).
- Setting a combination again (with the same `up`) replaces the earlier bind. A press bind and a release bind of the same combination can exist side by side.
- When several mods bind the same combination, only the bind of the mod loaded first fires.
- Binds are checked after every [`input.on_key`](input.md#inputon_key) filter, and they fire even when a filter took the key.
- A line runs after the mod phase, as if typed: a mod command (`//name`, or `/name` when a mod registered `name`) runs that command, any other line runs as a game command. Only a line starting with `//` runs without the `actions` permission; without it, any other line does not run and the chat log says "<mod>: the bind <key> runs a game command, which needs actions."
- A function runs as the player's input: it may sleep, and game actions in it work with the `actions` permission.

## `binds.unset`

```lua
binds.unset(key)
```

Removes this mod's binds of a key combination, both the press bind and the release bind. Combinations are compared by meaning, so `"ctrl+a"` and `"^a"` are the same.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `key` | `string` | yes | The [key combination](input.md#key-combinations) to unbind. |

**Returns** nothing. A combination the mod has not bound is ignored.

**Errors** `no key "<key>" (e.g. "f1", "ctrl+a", "^!1", "numpad+")` when `key` is not a known combination.

**Example**

```lua
binds.set("f11", "//mymod panic")
commands.register("nobind", function()
  binds.unset("f11")
  print("F11 is the game's again")
end)
```

## `binds.list`

```lua
binds.list() -> table[]
```

This mod's binds, in the order they were set (a replaced bind moves to the end). Other mods' binds are not included.

**Returns** `table[]`: one table per bind:

| Field | Type | Description |
| --- | --- | --- |
| `key` | `string` | The combination as it was written in `binds.set`. |
| `line` | `string?` | The bound line; `nil` when the bind calls a function. |
| `up` | `boolean` | It fires on the release. |
| `typing` | `boolean` | It fires while a text field is open too. |

**Example**

```lua
for _, b in ipairs(binds.list()) do
  print(b.key .. " -> " .. (b.line or "(a function)") .. (b.up and " on release" or ""))
end
```
