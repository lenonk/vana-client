# `packets`

The `packets` namespace lets a mod watch, change, block and send the packets that travel between the client and the game server. A hook added with `packets.on_in` sees each packet from the server before the client reads it; one added with `packets.on_out` sees each packet the client is about to send. A hook can return new bytes to replace the packet, or `false` to block it. Watching needs the `packets.read` permission; changing, blocking and injecting packets need `packets.write`. Hooks run inside the client's packet handling, not in the mod phase: each call must finish within 20 ms and cannot `sleep`, but it can read game state. Every packet is a Lua string of raw bytes that starts with the 4-byte header; see [Packet bytes](#packet-bytes) for how to read and write one.

```lua
-- count the packets the server sends, by id
local counts = {}
packets.on_in(nil, function(p)
  counts[p.id] = (counts[p.id] or 0) + 1
end)

commands.register("pcount", function()
  for id, n in pairs(counts) do print(("0x%03X: %d"):format(id, n)) end
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`packets.on_in(id, handler)`](#packetson_in) | Hooks packets from the server, before the client reads them. |
| [`packets.on_out(id, handler)`](#packetson_out) | Hooks packets the client is about to send to the server. |
| [`packets.off(hook)`](#packetsoff) | Removes a hook. |
| [`packets.inject_in(id_or_data, body?)`](#packetsinject_in) | Hands the client a packet as if the server had sent it. |
| [`packets.inject_out(id_or_data, body?)`](#packetsinject_out) | Sends a packet to the server. |
| [`packets.build(id, body)`](#packetsbuild) | Makes a whole packet (header and body) from an id and a body. |
| [`packets.last(direction, id)`](#packetslast) | The last packet of an id that came in or went out. |

## How hooks work

A hook is a function that gets one table, `p` (a [Packet](types.md#packet)):

| Field | Type | Description |
| --- | --- | --- |
| `id` | `integer` | The packet's id (0 to 0x1FF), as the packet arrived at the hooks. |
| `data` | `string` | The packet's bytes as they are now, header included: what an earlier hook returned, otherwise as the packet arrived. |
| `original` | `string` | The bytes as they arrived, before any hook changed them. |
| `injected` | `boolean` | `true` when a mod made this packet with `packets.inject_in` or `packets.inject_out`. |
| `blocked` | `boolean` | `true` when an earlier hook has blocked it. |
| `outgoing` | `boolean` | `false` in `on_in` hooks, `true` in `on_out` hooks. |

What the hook returns decides what happens to the packet:

| The hook returns | Effect |
| --- | --- |
| nothing, `nil`, `true` or any other value not below | The packet goes on as it is. |
| `false` | The packet is blocked: an incoming one is never read by the client, an outgoing one is never sent. Needs `packets.write`. |
| a string of 4 bytes or more | The string replaces the packet. Its header's size is set for you, and it is padded with zero bytes to a multiple of 4. Its id is read from the new header, so keep the first two bytes' id unless you mean to change it. Needs `packets.write`. |

A returned string shorter than 4 bytes is ignored; one longer than 508 bytes raises "a packet is at most 508 bytes (<n> returned): kept as it was", and the packet goes on unchanged. A mod without `packets.write` that returns `false` or a string gets the error "changing or blocking packets needs packets.write (/lua grant)", and the packet goes on unchanged. A hook that raises an error leaves the packet unchanged too.

The hooks form a chain. Every running mod's hooks run in the order the mods were loaded, and a mod's own hooks in the order it added them. Each hook sees the packet as the hooks before it left it. A blocked packet still goes through the rest of the chain, with `blocked = true`, and stays blocked: a later hook cannot let it through again.

A hook runs inline, inside the client's packet handling: it cannot call `sleep`, `sleep_frames` or `wait_until`, and it stops with the error "ran past its time budget" after 20 ms. It may read game state, and calls that queue work (such as `chat.add`) take effect after the next mod phase.

## `packets.on_in`

```lua
packets.on_in(id, handler) -> integer
```

Adds a hook on the packets the server sends, called before the client reads each one. Use it to react to a packet, or (with `packets.write`) to change or hide it.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` or `nil` | yes (may be `nil`) | The packet id to hook, such as `0x017`; `nil` hooks every packet. Pass it as a number: any value that is not a number hooks every packet too. |
| `handler` | `function(p)` | yes | The hook; see [How hooks work](#how-hooks-work) for `p` and the return value. |

**Returns** `integer`: the hook's id, for `packets.off`.

**Needs** the `packets.read` permission to add the hook; the hook needs `packets.write` to change or block a packet.

**Errors** without `packets.read`; when `handler` is missing (pass `nil` as the id to hook every packet: `packets.on_in(nil, fn)`).

**Example**

```lua
-- hide chat lines while "quiet" is on (0x017 is the chat packet)
local quiet = false
commands.register("quiet", function()
  quiet = not quiet
  print("quiet: " .. tostring(quiet))
end)

packets.on_in(0x017, function(p)
  if quiet then return false end  -- needs packets.write
end)
```

## `packets.on_out`

```lua
packets.on_out(id, handler) -> integer
```

Adds a hook on the packets the client has built and is about to send to the server. It works as [`packets.on_in`](#packetson_in) does, with `p.outgoing = true`; a blocked packet is not sent.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` or `nil` | yes (may be `nil`) | The packet id to hook; `nil` hooks every packet. |
| `handler` | `function(p)` | yes | The hook; see [How hooks work](#how-hooks-work). |

**Returns** `integer`: the hook's id, for `packets.off`.

**Needs** the `packets.read` permission to add the hook; the hook needs `packets.write` to change or block a packet.

**Errors** as for [`packets.on_in`](#packetson_in).

**Example**

```lua
-- log every packet the client sends, with its size
packets.on_out(nil, function(p)
  log.debug(("out 0x%03X, %d bytes"):format(p.id, #p.data))
end)
```

The header's sequence bytes (bytes 2 and 3) of an outgoing packet are filled in when the packet is sent, so whatever they hold in a hook is replaced.

## `packets.off`

```lua
packets.off(hook)
```

Removes a hook that `packets.on_in` or `packets.on_out` added.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `hook` | `integer` | yes | The id `packets.on_in` or `packets.on_out` returned. |

**Returns** nothing. An id that is not a hook of this mod is ignored.

**Example**

```lua
-- react to the next zone-in packet (0x00A) only once
local hook
hook = packets.on_in(0x00A, function(p)
  print("zone-in packet: " .. #p.data .. " bytes")
  packets.off(hook)
end)
```

## `packets.inject_in`

```lua
packets.inject_in(id_or_data, body?)
```

Hands the client a packet as if the server had sent it. Give either an id and the body (the bytes after the header: the header is made for you, as [`packets.build`](#packetsbuild) makes it), or one string holding a whole packet with its header.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id_or_data` | `integer` or `string` | yes | The packet id (0 to 0x1FF), or a whole packet (4 to 508 bytes, header first). |
| `body` | `string` | with an id | The body after the 4-byte header, at most 504 bytes. Leave it out when `id_or_data` is a whole packet. |

**Returns** nothing.

**Needs** the `packets.write` permission.

**Errors** "packet id <id>: ids are 0 to 0x1FF" for an id out of range; "a packet body is at most 504 bytes (<n> given)" for a longer body; "a whole packet is 4 to 508 bytes" for a whole packet of another size; `inject takes (id, body) or a whole sub-packet` when the arguments are neither an id with a body nor one string.

**Example**

```lua
-- show a tell from "Helper" (chat packet 0x017: kind at byte 4,
-- the sender's name at byte 8 in 15 bytes, the text from byte 23)
local name, text = "Helper", "Hello from a mod"
local body = string.pack("<BBI2", 3, 0, 0)
  .. name .. string.rep("\0", 15 - #name)
  .. text .. "\0"
packets.inject_in(0x017, body)
```

The packet is queued. After this frame's mod phase it goes through every mod's `on_in` hooks (with `injected = true`), and then the client reads it. A whole packet keeps its id, but its header's size field is set from its bytes (padded to a multiple of 4), so a wrong size can't throw off the packets after it; `packets.build` makes the header for you.

## `packets.inject_out`

```lua
packets.inject_out(id_or_data, body?)
```

Sends a packet to the server, as if the client had built it. It takes the same arguments as [`packets.inject_in`](#packetsinject_in).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id_or_data` | `integer` or `string` | yes | The packet id (0 to 0x1FF), or a whole packet (4 to 508 bytes, header first). |
| `body` | `string` | with an id | The body after the 4-byte header, at most 504 bytes. |

**Returns** nothing.

**Needs** the `packets.write` permission.

**Errors** as for [`packets.inject_in`](#packetsinject_in).

**Example**

```lua
-- say a line through the chat packet 0x0B5: the chat mode (0, say), a 0 byte, then the text
local text = "Hello!"
packets.inject_out(0x0B5, string.pack("<BB", 0, 0) .. text .. "\0")
```

The packet is queued. After this frame's mod phase it goes through every mod's `on_out` hooks (with `injected = true`) and is then sent. Send only packets whose layout you know: the client does not check them.

## `packets.build`

```lua
packets.build(id, body) -> string
```

Makes a whole packet from an id and a body: the 4-byte header (the id, the size, a sequence of 0) followed by the body, padded with zero bytes to a multiple of 4. Use it to prepare a packet to inject or to return from a hook.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The packet id, 0 to 0x1FF. |
| `body` | `string` | yes | The bytes after the header. At most 504 bytes: the header cannot describe a longer packet. |

**Returns** `string`: the whole packet.

**Errors** "packet id <id>: ids are 0 to 0x1FF"; "a packet body is at most 504 bytes (<n> given)".

**Example**

```lua
local p = packets.build(0x0B5, "\0\0Hi\0")
print(#p)                                --> 12 (4 header + 5 body, padded to 12)
print(string.unpack("<I2", p) & 0x1FF)   --> 181, which is 0x0B5
```

No permission is needed: building a packet does not send it.

## `packets.last`

```lua
packets.last(direction, id) -> string?
```

The last packet of an id that came in from the server or went out to it, as it arrived at the hooks (before any hook changed it), header included. Every mod shares the same record.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `direction` | `string` | yes | `"in"` for packets from the server, `"out"` for packets to it. Any value but `"out"` means `"in"`. |
| `id` | `integer` | yes | The packet id. |

**Returns** `string?`: the whole packet, or `nil` when none of that id has been recorded.

**Needs** the `packets.read` permission.

**Example**

```lua
-- packets are recorded only while some mod hooks that direction
packets.on_in(0x00D, function() end)

commands.register("lastpc", function()
  local p = packets.last("in", 0x00D)
  print(p and ("last 0x00D: " .. #p .. " bytes") or "none yet")
end)
```

Packets are recorded only while at least one running mod has a hook in that direction (`on_in` for `"in"`, `on_out` for `"out"`), and for packets mods inject. Add a hook that does nothing, as above, if your mod relies on `packets.last`.

## Packet bytes

Every packet a mod sees or makes is a Lua string holding the whole packet, header first:

| Bytes (counting from 0) | Lua string positions | What they hold |
| --- | --- | --- |
| 0 and 1 | 1 and 2 | A little-endian 16-bit word: the id in its low 9 bits, the packet's size in 4-byte units in its high 7 bits. |
| 2 and 3 | 3 and 4 | A sequence number, little-endian (incoming: the server's; outgoing: filled in when sent). |
| 4 onward | 5 onward | The body. |

A packet's size is always a multiple of 4 bytes, padded with zero bytes. Packet layouts are usually written with offsets counted from the start of the header (byte 0), while Lua counts string positions from 1: the field at offset `n` is at position `n + 1`. `string.unpack` and `string.pack` read and write the fields; `<` means little-endian, `B` is 1 byte, `I2` 2 bytes, `I4` 4 bytes, `z` a string ended by a zero byte.

**Reading a packet**

```lua
packets.on_in(0x017, function(p)
  local word, seq = string.unpack("<I2I2", p.data)
  local id = word & 0x1FF                -- the same as p.id
  local size = (word >> 9) * 4           -- in bytes, header included
  local kind = string.unpack("B", p.data, 4 + 1)
  local sender = string.unpack("z", p.data:sub(8 + 1, 8 + 15) .. "\0")
  print(("0x%03X, %d bytes, seq %d, kind %d, from %s"):format(id, size, seq, kind, sender))
end)
```

**Changing a packet**

```lua
-- replace the text of chat lines that mention "spoiler" (the text starts at offset 23)
packets.on_in(0x017, function(p)
  local text = string.unpack("z", p.data .. "\0", 23 + 1)
  if text:find("spoiler", 1, true) then
    return p.data:sub(1, 23) .. "(hidden)\0"  -- needs packets.write; the size is fixed for you
  end
end)
```

Text in packets is in the game's encoding (Shift-JIS), not UTF-8; plain ASCII letters and digits are the same in both.
