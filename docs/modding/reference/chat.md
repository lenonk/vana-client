# `chat`

The chat log and the chat line. `chat.add` writes lines into the log, in a chat mode's colour if you like;
`chat.icon` turns a PNG into a character that draws as an image inside chat text; `chat.line` and `chat.set_line`
read and fill the line the player types in. `chat.add` and `chat.set_line` are queued and take effect after the mod
phase. In the lobby there is no log: `chat.add` lines go to vana-client.log. No permission is needed.

```lua
chat.add("Ready to pull!", 13)   -- in the party chat colour
```

## Functions

| Function | What it does |
| --- | --- |
| [`chat.add(text, how?)`](#chatadd) | Adds a line to the chat log, optionally in a chat mode. |
| [`chat.icon(path)`](#chaticon) | Makes a PNG from the mod's folder into an inline chat icon character. |
| [`chat.line()`](#chatline) | Whether the chat line is open, and its text. |
| [`chat.set_line(text?, open?)`](#chatset_line) | Opens the chat line with some text, or closes it. |

## `chat.add`

```lua
chat.add(text, how?)
```

Adds a line to the chat log. Without `how` the line is white and has no mode. With a chat mode, the line takes that
mode's colour and indent and shows in the log windows that show that mode, as a game line of that mode would.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `text` | `string` | yes | The line. Auto-translate phrases in it (from `text.phrase` or `text.autotranslate`) are shown as the game shows them. |
| `how` | `integer` or `table` | no | A chat mode, 0..255; or a table, see below. |

**`how` fields** (when it is a table)

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `mode` | `integer` | none | The chat mode, 0..255, as when `how` is a number. When both fields are given, `mode` wins. |
| `colour` | `integer` | none | A chat mode, 0..255, whose colour the line takes. The line itself has no mode, so the log windows' mode settings do not apply to it: it shows in the first log window, as lines without a mode do. |

Some modes the client prints received chat in: 9 say, 10 shout, 11 yell, 12 tell, 13 party, 14 linkshell, 15 emote,
200 system message.

**Returns** nothing.

**Errors** when a table's `mode` or `colour` is not a whole number from 0 to 255.

**Example**

```lua
chat.add("A plain white line")
chat.add("Coloured and filed like a party line", 13)
chat.add("Tell-coloured, without the tell mode", { colour = 12 })
```

The line passes the chat filters (`filters.chat`) like any other, with its mode and `battle` false.

## `chat.icon`

```lua
chat.icon(path) -> string
```

Loads a PNG from the mod's folder as an inline icon and returns the one character that stands for it. Put that
character in text for `chat.add` or in a chat filter's replacement, and the image is drawn where it stands, at the
text's line height (keeping its proportions), in both UIs' logs and in the classic UI's game-font text.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `path` | `string` | yes | The PNG's path inside the mod's folder, e.g. `"icons/star.png"`. |

**Returns** `string`: a one-character string. The same path always gives the same character.

**Errors**

- `"<path>": outside the mod's folder` for a path with `..`, starting with `/`, or containing `:`.
- `<path>: <reason>` when the file cannot be read, and `<path>: <reason> (a PNG)` when it is not a PNG.
- "bundled mods carry no image files" for a mod built into the client.
- "too many chat icons" once 4096 icons are registered (all mods together).

**Example**

```lua
local star = chat.icon("icons/star.png")
chat.add(star .. " Rare item dropped!")
```

An icon is read once: if you change the PNG, the client keeps showing the first image until it restarts.

## `chat.line`

```lua
chat.line() -> table
```

The chat line the player types in: whether it is open, and what is typed in it so far.

**Returns** `table`:

| Field | Type | Description |
| --- | --- | --- |
| `open` | `boolean` | Whether the chat line is open. |
| `text` | `string` | Its text; `""` when it is closed. |

**Errors** "game state is not available here (in the lobby, or outside the mod phase)" in the lobby or a chat filter.

**Example**

```lua
local line = chat.line()
if line.open and line.text ~= "" then
  print("The player is typing: " .. line.text)
end
```

## `chat.set_line`

```lua
chat.set_line(text?, open?)
```

Opens the chat line with `text` in it, in place of whatever was typed; the player still presses Enter to send it. With
`open` false it closes the chat line and clears it.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `text` | `string` | no, default `""` | The text to put in the line. |
| `open` | `boolean` | no, default `true` | `false` closes the line instead (the text is then ignored). |

**Returns** nothing.

**Example**

```lua
-- F10 starts a tell: the player types the name and message.
binds.set("f10", function()
  chat.set_line("/tell ")
end)
```
