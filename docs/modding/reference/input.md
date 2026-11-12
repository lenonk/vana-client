# `input`

The `input` namespace sees the keyboard and the mouse before the game and its windows do, can take an event so the game never sees it, and can press keys for the player. Watching is free: `on_key`, `on_mouse` and the reads (`held`, `modifiers`, `mouse`, `typing`) need no permission. Taking an event (returning `true` from a filter) and pressing keys (`press`, `send`) need the `input.capture` permission. The filters run only while the character is in the world. For a fixed key that runs a command or a function, [`binds`](binds.md) is simpler. Key names are listed under [Key names](#key-names) and combinations such as `"ctrl+f1"` under [Key combinations](#key-combinations).

```lua
-- take Ctrl+M away from the game and use it ourselves
input.on_key(function(e)
  if e.key == "m" and e.ctrl and e.down and not e.typing then
    print("Ctrl+M pressed")
    return true  -- needs input.capture
  end
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`input.on_key(handler)`](#inputon_key) | Sees every key press, repeat and release first, and can take it. |
| [`input.on_mouse(handler)`](#inputon_mouse) | Sees the mouse's buttons, movement and wheel first, and can take them. |
| [`input.on_pad(handler)`](#inputon_pad) | Sees the game controller's buttons first, and can take them. |
| [`input.pad()`](#inputpad) | The game controller's buttons, sticks and triggers now. |
| [`input.off(id)`](#inputoff) | Removes a key, mouse or pad filter. |
| [`input.held(key)`](#inputheld) | Whether a key is held down. |
| [`input.modifiers()`](#inputmodifiers) | Which of Ctrl, Alt, Shift and Win are held. |
| [`input.mouse()`](#inputmouse) | The pointer's position and which buttons are held. |
| [`input.typing()`](#inputtyping) | Whether a text field has the keyboard. |
| [`input.press(key)`](#inputpress) | Taps a key, with modifiers, for the game. |
| [`input.send(key, down)`](#inputsend) | Holds a key down for the game, or lets it go. |

## `input.on_key`

```lua
input.on_key(handler) -> integer
```

Adds a key filter: `handler` is called for every key press, auto-repeat and release, before the game and its windows see it. Return `true` to take the key from them.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `handler` | `function(e)` | yes | Called with a [KeyEvent](types.md#keyevent) `e`; returns `true` to take the key. |

**`e` fields**

| Field | Type | Description |
| --- | --- | --- |
| `key` | `string` | The key's name, in lower case: `"a"`, `"1"`, `"f1"`, `"enter"`, `"numpad+"`, `"lctrl"`... (see [Key names](#key-names)). |
| `down` | `boolean` | `true` for a press or a repeat, `false` for a release. |
| `repeated` | `boolean` | `true` when the key is held and repeating. |
| `ctrl` | `boolean` | Ctrl (either one) is held. |
| `alt` | `boolean` | Alt (either one) is held. |
| `shift` | `boolean` | Shift (either one) is held. |
| `typing` | `boolean` | A text field (the chat line, a search box...) has the keyboard. |
| `blocked` | `boolean` | An earlier filter, of this mod or another, has already taken the key. |

**Returns** `integer`: the filter's id, for `input.off`.

**Needs** the `input.capture` permission to take a key (return `true`). Watching needs none. Without it, returning `true` gives the error "taking a key or the mouse needs input.capture" and the key goes on to the game.

**Example**

```lua
input.on_key(function(e)
  if e.down and not e.repeated then
    print("pressed " .. e.key .. (e.shift and " with shift" or ""))
  end
end)
```

Notes:
- Taking a press takes the rest of that keystroke too. Its repeats are taken without calling any filter, and its release is shown to the filters but goes no further.
- Every running mod's filters run, in the order the mods were loaded, and a mod's in the order it added them. A key one filter took still reaches the later ones, with `blocked = true`.
- [Binds](binds.md) are checked after the filters.
- A filter is the player's input: game actions in it work with the `actions` permission (`automation` is not needed).
- A filter runs inside the client's input handling, with 20 ms to finish. It is called as a coroutine, but if it sleeps, what it returns afterwards is ignored and the key is not taken.
- Key names are positions on the keyboard, named after a US layout: `"q"` is the key where Q is on a US keyboard, whatever is printed on it.
- Keys missing from [Key names](#key-names) (media keys, for example) are not reported.

## `input.on_mouse`

```lua
input.on_mouse(handler) -> integer
```

Adds a mouse filter: `handler` is called for every button press and release, every pointer movement and every turn of the wheel, before the game and its windows see it. Return `true` to take the event.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `handler` | `function(e)` | yes | Called with a [MouseEvent](types.md#mouseevent) `e`; returns `true` to take it. |

**`e` fields**

| Field | Type | Description |
| --- | --- | --- |
| `kind` | `string` | `"down"` (a button pressed), `"up"` (released), `"move"` (the pointer moved) or `"wheel"`. |
| `button` | `string?` | For `"down"` and `"up"`: `"left"`, `"right"`, `"middle"`, `"back"`, `"forward"` or `"other"`. `nil` otherwise. |
| `x` | `integer` | The pointer's position in window pixels, from the left. |
| `y` | `integer` | The pointer's position in window pixels, from the top. |
| `delta` | `number?` | For `"wheel"`: how far it turned, in notches, positive when turned up (away from you). It can be a fraction with a touchpad. `nil` otherwise. |
| `blocked` | `boolean` | An earlier filter has already taken this event. |

**Returns** `integer`: the filter's id, for `input.off`.

**Needs** the `input.capture` permission to take an event (return `true`). Watching needs none.

**Example**

```lua
input.on_mouse(function(e)
  if e.kind == "down" and e.button == "middle" then
    print(("middle click at %d, %d"):format(e.x, e.y))
    return true  -- needs input.capture
  end
end)
```

Taking a button press takes its release too (the filters still see the release). A left or right press on a [HUD](hud.md) element that has a click handler goes to that element; the filters still see the press and its release, with `blocked = true`. The chain of filters, the time limit and the player's-input rule are as for [`input.on_key`](#inputon_key).

## `input.on_pad`

```lua
input.on_pad(handler) -> integer
```

Calls `handler` for every press and release of a game controller button, before the game reads the pad. Return
`true` from a press to take the button: the game does not see it until it comes up (the filters still see its
release). Taking needs the `input.capture` permission. Filters chain across mods as key filters do: a taken press
still reaches the later ones with `blocked = true`. The pad works while Settings' gamepad is on and the window has
the focus (or Settings' background play is on).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `handler` | `function` | yes | Called as `handler(e)`; return `true` to take the press. |

`e` is a table:

| Field | Type | Description |
| --- | --- | --- |
| `button` | `string` | `"a"`, `"b"`, `"x"`, `"y"`, `"up"`, `"down"`, `"left"`, `"right"`, `"lb"`, `"rb"`, `"lt"`, `"rt"`, `"l3"`, `"r3"`, `"start"` or `"back"` (an Xbox layout; other pads by position). |
| `down` | `boolean` | `true` for a press, `false` for a release. |
| `blocked` | `boolean` | An earlier filter took it. |

**Returns** `integer`: an id for [`input.off`](#inputoff).

**Example**

```lua
-- While RT is held, the face buttons are ours: a crossbar.
input.on_pad(function(e)
  if e.down and input.pad().buttons.rt and (e.button == "a" or e.button == "b" or e.button == "x" or e.button == "y") then
    print("slot " .. e.button)
    return true
  end
end)
```

It is the player's input: actions may act from the handler, as from key filters.

## `input.pad`

```lua
input.pad() -> table
```

The game controller now: `{connected, buttons, lx, ly, rx, ry, lt, rt}`. `connected` is whether a pad plays (Settings'
gamepad on, the window focused or background play on); `buttons` maps each button name of
[`input.on_pad`](#inputon_pad) to whether it is held (taken buttons too); `lx`, `ly`, `rx`, `ry` are the sticks, -1 to
1 with up positive; `lt` and `rt` the triggers, 0 to 1.

**Returns** `table`.

**Example**

```lua
local p = input.pad()
if p.connected and p.buttons.lt then print("left trigger held") end
```

## `input.off`

```lua
input.off(id)
```

Removes a key, mouse or pad filter.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The id `input.on_key`, `input.on_mouse` or `input.on_pad` returned. |

**Returns** nothing. An id that is not a filter of this mod is ignored.

**Example**

```lua
local filter = input.on_key(function(e) print(e.key) end)
timer.after(10, function()
  input.off(filter)  -- watch the keys for ten seconds only
end)
```

## `input.held`

```lua
input.held(key) -> boolean
```

Whether a key is held down right now.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `key` | `string` | yes | A [key name](#key-names) or alias. `"shift"`, `"ctrl"` (or `"control"`), `"alt"` and `"win"` (or `"super"`) mean either the left or the right key. |

**Returns** `boolean`: `true` while the key is down. An unknown name gives `false`.

**Example**

```lua
timer.every(0.5, function()
  if input.held("shift") and input.held("f1") then
    print("Shift+F1 held")
  end
end)
```

It follows the real keyboard: keys pressed with `input.press` or `input.send` do not count. When the game window loses the keyboard focus, every key counts as released.

## `input.modifiers`

```lua
input.modifiers() -> table
```

Which modifier keys are held now.

**Returns** `table`: `{ ctrl, alt, shift, win }`, each a `boolean`, `true` when either the left or the right key is held.

**Example**

```lua
local m = input.modifiers()
if m.ctrl and m.shift then print("Ctrl+Shift held") end
```

## `input.mouse`

```lua
input.mouse() -> table
```

Where the pointer is and which buttons are held.

**Returns** `table`:

| Field | Type | Description |
| --- | --- | --- |
| `x` | `integer` | The pointer's position in window pixels, from the left. |
| `y` | `integer` | The pointer's position in window pixels, from the top. |
| `left` | `boolean` | The left button is held. |
| `right` | `boolean` | The right button is held. |
| `middle` | `boolean` | The middle button is held. |

**Example**

```lua
local m = input.mouse()
print(("pointer at %d, %d, left button %s"):format(m.x, m.y, m.left and "down" or "up"))
```

## `input.typing`

```lua
input.typing() -> boolean
```

Whether a text field has the keyboard: the chat line, a search field, a name being typed, and so on. Check it before acting on a letter key, so you do not act while the player is typing.

**Returns** `boolean`: `true` while a text field is open.

**Errors** `game state is not available here` in the lobby.

**Example**

```lua
input.on_key(function(e)
  if e.key == "f" and e.down and not input.typing() then
    print("F pressed outside a text field")
  end
end)
```

Inside a key filter, `e.typing` gives the same answer.

## `input.press`

```lua
input.press(key)
```

Taps a key for the game: the key and the modifiers given count as pressed for one frame, as if the player had pressed and released them. Use it to work a game function that has a key but no command.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `key` | `string` | yes | A [key combination](#key-combinations): `"enter"`, `"ctrl+f1"`, `"^1"` (Ctrl+1), `"alt+shift+2"`. |

**Returns** nothing.

**Needs** the `input.capture` permission.

**Errors** `no key "<key>" (e.g. "f1", "ctrl+a", "^!1", "numpad+")` when the key is not a known combination; `"<key>": the Windows key cannot be pressed for the game` for a combination with Win.

**Example**

```lua
commands.register("confirm", function()
  input.press("enter")
end)
```

The press is queued and given to the game and its windows after this frame's mod phase. It goes to them directly: no mod's key filters or binds see it. The game reads no Windows key, so a combination with Win raises an error. A chat line submitted by a pressed Enter runs as the mod's line, not the player's (see [commands.run](commands.md#commandsrun)).

## `input.send`

```lua
input.send(key, down)
```

Holds a key down for the game (`down = true`) or lets it go (`down = false`). The key stays down until you send it up, or until the mod unloads (its held keys then come up). Use it for keys that do something while held.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `key` | `string` | yes | A [key name](#key-names) or alias, without modifiers. `"shift"`, `"ctrl"`, `"alt"` and `"win"` mean the left key. |
| `down` | `boolean` | yes | `true` to hold it, `false` to let it go. |

**Returns** nothing.

**Needs** the `input.capture` permission.

**Errors** `no key "<key>"` when the name is not a key (a combination such as `"ctrl+a"` is not accepted here).

**Example**

```lua
commands.register("walk", function()
  input.send("w", true)   -- hold W for two seconds
  sleep(2)
  input.send("w", false)
end)
```

Like `input.press`, it is queued until after the mod phase and goes to the game directly, past every mod's filters and binds. Always send the key up again.

## Key names

These are the names `on_key` reports and the names every function here and in [`binds`](binds.md) accepts. Upper and lower case do not matter, and spaces around the name are ignored.

| Group | Names |
| --- | --- |
| Letters | `a` … `z` |
| Digits (the row above the letters) | `0` … `9` |
| Function keys | `f1` … `f12` |
| Editing and moving | `escape`, `enter`, `space`, `tab`, `backspace`, `insert`, `delete`, `home`, `end`, `pageup`, `pagedown`, `up`, `down`, `left`, `right` |
| Punctuation | `-`, `=`, `[`, `]`, `\`, `;`, `'`, `` ` ``, `,`, `.`, `/` |
| Number pad | `numpad0` … `numpad9`, `numpad+`, `numpad-`, `numpad*`, `numpad/`, `numpad.`, `numpadenter` |
| Locks and others | `capslock`, `scrolllock`, `numlock`, `pause`, `printscreen`, `apps` (the menu key) |
| Modifiers | `lshift`, `rshift`, `lctrl`, `rctrl`, `lalt`, `ralt`, `lwin`, `rwin` |

Other names are accepted as aliases (but `on_key` always reports the names above):

| Alias | Same as |
| --- | --- |
| `return` | `enter` |
| `esc` | `escape` |
| `minus` | `-` |
| `equals`, `equal` | `=` |
| `pgup`, `pgdn` | `pageup`, `pagedown` |
| `ins`, `del` | `insert`, `delete` |
| `shift`, `ctrl`, `control`, `alt`, `win`, `super` | `lshift`, `lctrl`, `lctrl`, `lalt`, `lwin`, `lwin` (in `input.held`: either side) |

## Key combinations

`input.press`, [`binds.set`](binds.md#bindsset) and [`binds.unset`](binds.md#bindsunset) take a key with modifiers in front of it. Write the modifiers as words with a `+`, as symbols, or mixed:

| Modifier | Words | Symbol |
| --- | --- | --- |
| Ctrl | `ctrl+`, `control+` | `^` |
| Alt | `alt+` | `!` |
| Shift | `shift+` | `~` |
| Win | `win+`, `super+`, `cmd+` | `@` |

So `"ctrl+f1"`, `"^f1"`, `"alt+shift+1"`, `"!~1"` and `"Ctrl+Numpad+"` are all valid. The key comes last and is a [key name](#key-names). A symbol is a modifier only when something follows it: `"-"` is the minus key, but `"~"` or `"^"` alone is not a key.
