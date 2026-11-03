# `log`

`log` writes lines to the client's log file, `vana-client.log` in the client's data folder (on Linux
`~/.local/share/vana-client/vana-client.log`), without showing them in the game. Use it for details you want when
debugging a mod; use `print` for what the player should see. Each line is written at once, prefixed with
`lua <mod id>: `. The functions work anywhere (also in the lobby and in filters) and need no permission.

```lua
events.on("zone_change", function(zone, previous)
  log.info("zone " .. tostring(previous) .. " -> " .. zone)
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`log.info(text)`](#loginfo) | Writes an ordinary line. |
| [`log.warn(text)`](#logwarn) | Writes a warning. |
| [`log.error(text)`](#logerror) | Writes an error. |
| [`log.debug(text)`](#logdebug) | Writes a debug line (only kept when debug logging is on). |

## `log.info`

```lua
log.info(text)
```

Writes `text` to vana-client.log as an ordinary (info) line.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `text` | `string` | yes | The line. A number is turned into text. |

**Returns** nothing.

**Errors** A `text` that is not a string or a number (for example `nil` or a table) raises an error: use `tostring`.

**Example**

```lua
local p = game.player()
log.info(string.format("%s at %.1f, %.1f", p.name, p.pos.x, p.pos.z))
```

## `log.warn`

```lua
log.warn(text)
```

Writes `text` to vana-client.log as a warning.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `text` | `string` | yes | The line. A number is turned into text. |

**Returns** nothing.

**Errors** As [`log.info`](#loginfo).

**Example**

```lua
local raw = fs.read("config.json")
if not raw then
  log.warn("no config.json, using defaults")
end
```

## `log.error`

```lua
log.error(text)
```

Writes `text` to vana-client.log as an error. It does not stop the mod or count as one of its errors.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `text` | `string` | yes | The line. A number is turned into text. |

**Returns** nothing.

**Errors** As [`log.info`](#loginfo).

**Example**

```lua
local ok, err = pcall(json.decode, fs.read("config.json") or "")
if not ok then
  log.error("config.json: " .. tostring(err))
end
```

## `log.debug`

```lua
log.debug(text)
```

Writes `text` to vana-client.log as a debug line. Debug lines are dropped unless the client runs with debug logging,
for example started with the environment variable `RUST_LOG=debug`.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `text` | `string` | yes | The line. A number is turned into text. |

**Returns** nothing.

**Errors** As [`log.info`](#loginfo).

**Example**

```lua
events.on("frame", function(dt)
  if dt > 0.1 then
    log.debug("slow frame: " .. dt .. " s")
  end
end)
```
