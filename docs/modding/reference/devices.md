# `devices`

The `devices` namespace reads and writes USB HID devices by their raw reports: the extra keys on some keyboards, macro pads, button boxes and similar hardware the game knows nothing about. List the devices, open one by its path, and its input reports arrive in a `data` handler in later frames; send output reports (to light a key or change a display) with `devices.write`. Every function except `devices.close` needs the `devices` permission. On Linux the devices are the `/dev/hidraw*` nodes, and the player must be allowed to read and write the node (for example through a udev rule); on Windows they are the system's HID interfaces, where keyboards and mice cannot be opened because the system keeps them. On other systems the list is empty.

```lua
for _, d in ipairs(devices.list()) do
  if d.vendor == 0x1234 and d.product == 0x5678 then
    devices.open(d.path, {
      data = function(id, report) print("report of " .. #report .. " bytes") end,
      close = function(id, err) print("device closed", err) end,
    })
    break
  end
end
```

## Functions

| Function | What it does |
| --- | --- |
| [`devices.list()`](#deviceslist) | The HID devices on this computer. |
| [`devices.open(path, handlers)`](#devicesopen) | Opens a device; its reports go to the handlers. |
| [`devices.write(id, report)`](#deviceswrite) | Sends an output report to an open device. |
| [`devices.close(id)`](#devicesclose) | Closes a device. |

## `devices.list`

```lua
devices.list() -> table[]
```

Lists the HID devices. A device with several interfaces (a keyboard with media keys, say) is listed once for each, with the same vendor and product ids; `interface` tells them apart. On Linux the list is in the order of the hidraw numbers.

**Returns** `table[]`: one table per device, with these fields:

| Field | Type | Description |
| --- | --- | --- |
| `path` | `string` | What to pass to `devices.open`: `/dev/hidrawN` on Linux, the interface's path on Windows. |
| `vendor` | `integer` | The USB vendor id; 0 when it could not be read. |
| `product` | `integer` | The USB product id; 0 when it could not be read. |
| `name` | `string` | The product's name; `""` when it could not be read. |
| `interface` | `integer?` | The USB interface number; `nil` when unknown. |
| `descriptor` | `string` | The report descriptor's raw bytes on Linux (it describes the device's reports); an empty string on Windows or when it could not be read. |

**Needs** the `devices` permission.

**Example**

```lua
for _, d in ipairs(devices.list()) do
  print(("%04x:%04x  interface %s  %s  (%s)"):format(
    d.vendor, d.product, tostring(d.interface), d.name, d.path))
end
```

## `devices.open`

```lua
devices.open(path, handlers) -> integer
```

Opens a device for reading and writing and returns its id at once. What happens next arrives in the handlers in later frames: `open` when it is open, `data` for each input report, `close` when it fails.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `path` | `string` | yes | A `path` from `devices.list`; any other path is refused. |
| `handlers` | `table` | yes | The handler functions, all optional. |

**`handlers` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `open` | `function(id)` | none | The device is open. |
| `data` | `function(id, report)` | none | One input report (a string of raw bytes): one call per report. When the device numbers its reports, the report id is the first byte. |
| `close` | `function(id, err)` | none | The device could not be opened (`err` is `"<path>: <reason>"`, such as a permission error), or reading it failed (for example, it was unplugged). |

**Returns** `integer`: the device's id, for `devices.write` and `devices.close`.

**Needs** the `devices` permission.

**Errors** "<path>: not a HID device (devices.list gives them)" for a path `devices.list` does not return.

**Example**

```lua
local function hex(s)
  return (s:gsub(".", function(c) return ("%02x "):format(c:byte()) end))
end

local first = devices.list()[1]
local pad = first and devices.open(first.path, {
  open = function(id) print("pad open") end,
  data = function(id, report) print("report: " .. hex(report)) end,
  close = function(id, err) print("pad closed: " .. tostring(err)) end,
})
```

A failure to open is not an error raised by the call: it arrives in `close`. The handlers run as normal handlers: they may sleep and read game state.

## `devices.write`

```lua
devices.write(id, report)
```

Sends an output report to an open device. The first byte is the report id, or 0 when the device does not number its reports. On Windows the string must be exactly as long as the device's report. The write is queued and done in the background.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The id `devices.open` returned. |
| `report` | `string` | yes | The report's bytes, report id first. |

**Returns** nothing.

**Needs** the `devices` permission.

**Errors** `no device <id>` when no open device has this id; `the device is closed` when the device can no longer be written (after a write failed, writing stops for good).

**Example**

```lua
local dev = devices.open("/dev/hidraw3", {})
-- report id 0, then the bytes this device expects (here: light key 1 in red)
devices.write(dev, string.char(0x00, 0x01, 0xff, 0x00, 0x00))
```

What the bytes after the report id mean depends on the device; its report descriptor (from `devices.list`) and its maker's documentation tell.

## `devices.close`

```lua
devices.close(id)
```

Closes a device. Its handlers are let go: nothing more arrives for it, and its `close` handler is not called. It is the same as [`net.close`](net.md#netclose).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The id `devices.open` returned. |

**Returns** nothing. An id that is not open is ignored.

**Example**

```lua
local dev = devices.open("/dev/hidraw3", {})
events.on("unload", function()
  devices.close(dev)
end)
```

Devices are also closed when the mod unloads.
