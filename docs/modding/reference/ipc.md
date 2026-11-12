# `ipc`

The `ipc` namespace lets copies of the same mod running in several game windows on one computer talk to each other: one window can tell the others to do something, or share what it sees. A message is a string (turn a table into one with `json.encode`). It goes to the same mod (the same mod id) in every other game window, which receives it as the `ipc_message` event. Sending and receiving both need the `multibox` permission. Lobby mods can send and receive before login too: a message from the lobby has an empty sender, and a message for a named character reaches only the window where that character is in the world.

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

Sends `text` to the same mod in every other game window on this computer, or only to the window playing `character`. The window that sends does not receive its message.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `text` | `string` | yes | The message. |
| `character` | `string` | no, default every window | Only the window whose character has this name (upper or lower case does not matter) handles it. |

**Returns** nothing.

**Needs** the `multibox` permission.

**Errors** `an ipc message is at most 65000 bytes encoded (<n> given)` for a longer message (the text is sent as JSON, so quotes and control characters take more room).

**Example**

```lua
-- tell the window playing "Tuxedo" what we are targeting
local me = game.player()
if me.target then
  ipc.send(json.encode({ cmd = "target", id = me.target }), "Tuxedo")
end
```

The message is queued and sent after this frame's mod phase. Delivery is not kept for later: a window whose copy of the mod is not running, or lacks `multibox`, never sees it.

## The `ipc_message` event

```lua
events.on("ipc_message", function(text, from) end)
```

Runs when the same mod in another game window sent a message with `ipc.send`.

| Argument | Type | Description |
| --- | --- | --- |
| `text` | `string` | The text given to `ipc.send`. |
| `from` | `string` | The name of the character playing the window that sent it; `""` when it was sent from the lobby. |

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
