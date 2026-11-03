# `settings`

`settings` reads the options a mod declares in its `mod.toml`. The client shows them to the user on the mod's Extras
page and keeps the user's choices; the mod reads the current value with `settings.get` and hears about changes through
the `settings_changed` event. `settings.get` works anywhere (also in the lobby) and needs no permission.

```lua
local function warn_below() return settings.get("warn_below") end

events.on("hpp_change", function(hpp, previous)
  if hpp < warn_below() and previous >= warn_below() then
    print("HP below " .. warn_below() .. "%!")
  end
end)
```

## Declaring settings

Each setting is a `[settings.<key>]` table in `mod.toml`; `<key>` is what `settings.get` takes. The settings appear on
the Extras page in the order they are written.

```toml
[settings.warn_below]
type = "int"
default = 50
min = 10
max = 90
step = 10
label = "Warn Below"
help = "Warn in the log when your HP drops below this percent."

[settings.sound]
type = "enum"
options = ["None", "Soft", "Loud"]
default = "Soft"
label = "Alert Sound"
scope = "character"
```

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `type` | string | `""` | `"bool"`, `"int"`, `"number"`, `"enum"`, `"string"`, `"colour"` or `"key"`. The Extras page shows `bool`, `enum`, `int` and `number` settings; the others are read with `settings.get` and set with `/lua set` only. |
| `default` | any TOML value | none | The value `settings.get` returns while the user has chosen none. |
| `label` | string | the key | The row's name on the Extras page. |
| `help` | string | `""` | The help text the Extras page shows for the row. |
| `scope` | string | `"account"` | `"account"`: one value for every character on this computer. `"character"`: a value per character. Any other value works as `"account"`. |
| `min` | number | `0` | The smallest value an `int` or `number` row offers. |
| `max` | number | `100` | The largest value an `int` or `number` row offers. |
| `step` | number | `1` for `int`, a twentieth of the range for `number` | The step between the values the row offers (at most 201 values). |
| `options` | string[] | `[]` | An `enum`'s choices. |

The value a row stores: `true` or `false` for `bool`; the chosen option's text for `enum`; an integer for `int`; a
number for `number`.

The user changes a setting on the Extras page (`/lua settings <mod>` opens it) or with `/lua set <mod> <key> <value>`,
where the value is read as JSON when it parses (`true`, `12`, `[1, 2]`, `"text"`) and as plain text otherwise. Either
way the mod gets the `settings_changed(key, value)` event. `/lua set` checks neither the type nor the range, and can
set a key mod.toml does not declare; `settings.get` reads such a key too.

## Functions

| Function | What it does |
| --- | --- |
| [`settings.get(key)`](#settingsget) | A setting's current value: the user's, else the declared default. |

## `settings.get`

```lua
settings.get(key) -> any
```

Returns the user's value for the setting, or the `default` from `mod.toml` when the user has set none. Read it when
you need it (it is cheap), so a change made while the mod runs is picked up.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `key` | `string` | yes | The setting's key (`<key>` in `[settings.<key>]`). |

**Returns** `any`: the value, as a boolean, number, string or table; `nil` for a setting declared without a `default` that the user has not set.

**Errors** "no setting "<key>" in mod.toml" when mod.toml declares no `[settings.<key>]`.

**Example**

```lua
events.on("settings_changed", function(key, value)
  if key == "sound" then
    print("Alert sound is now " .. tostring(value))
  end
end)
print("Alert sound: " .. settings.get("sound"))
```

In a `lobby` mod, `character`-scoped settings read as their defaults until a character logs in.
