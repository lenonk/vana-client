# `ipc`

The `ipc` namespace lets copies of the same mod running in several game windows on one computer talk to each other: one window can tell the others to do something, or share what it sees. A message is a string (turn a table into one with `json.encode`). It goes to the same mod (the same mod id) in every other game window, which receives it as the `ipc_message` event. Sending and receiving both need the `multibox` permission, and both windows must be logged in to the world: messages sent from the lobby are dropped, and a window that is not in the world yet does not receive them.

```lua
commands.register("all", function(...)
  ipc.send(table.concat({ ... }, " "))
end)

events.on("ipc_message", function(text, from)
  chat.add(from .. " says to all windows: " .. text)
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`ipc.send(text, character?)`](#ipcsend) | Sends text to this mod in the other game windows on this computer. |

The message arrives as the [`ipc_message`](#the-ipc_message-event) event.

## `ipc.send`

```lua
ipc.send(text, character?)
```

Sends `text` to the same mod in every other game window on this computer, or only to the window playing `character`. The window that sends does not receive its own message.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `text` | `string` | yes | The message. |
| `character` | `string` | no, default every window | Only the window whose character has this name (upper or lower case does not matter) handles it. |

**Returns** nothing.

**Needs** the `multibox` permission.

**Example**

```lua
-- tell the window playing "Tuxedo" what we are targeting
local me = game.player()
if me.target then
  ipc.send(json.encode({ cmd = "target", id = me.target }), "Tuxedo")
end
```

The message is queued and sent after this frame's mod phase. Delivery is not kept for later: a window that has not logged in, or whose copy of the mod lacks `multibox`, never sees it. Keep messages short (well under 64 KB); a message too large to send is lost without an error.

## The `ipc_message` event

```lua
events.on("ipc_message", function(text, from) end)
```

Runs when the same mod in another game window sent a message with `ipc.send`.

| Argument | Type | Description |
| --- | --- | --- |
| `text` | `string` | The text given to `ipc.send`. |
| `from` | `string` | The name of the character playing the window that sent it. |

The mod receives it only while it holds the `multibox` permission. The handler is a normal event handler: it may sleep and read game state. It is not the player's input, so a game action started from it (such as `commands.run` with a game command) needs the `automation` permission.

**Example**

```lua
events.on("ipc_message", function(text, from)
  local msg = json.decode(text)
  if msg.cmd == "target" then
    actions.target(msg.id)  -- a game action: needs automation here
    print("targeting for " .. from)
  end
end)
```
