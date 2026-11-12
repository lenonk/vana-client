# `text`

`text` converts between Lua strings and the game's text: Shift-JIS bytes (what packets and game files hold) and
auto-translate phrases. Lua strings in the API are UTF-8; an auto-translate phrase inside one is a short run of
special characters that the chat log shows as the phrase's words, so a phrase can be put in chat text like any other
text. `text.from_shift_jis`, `text.to_shift_jis` and `text.phrase` work anywhere; `text.plain` and
`text.autotranslate` need the game's dictionary and raise "game state is not available here" in the lobby and inside
filters. No permission is needed.

```lua
events.on("chat_message", function(line, sender)
  local plain = text.plain(line)
  if plain:lower():find("lfg", 1, true) then
    print(sender .. " is looking for a group: " .. plain)
  end
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`text.from_shift_jis(bytes)`](#textfrom_shift_jis) | Shift-JIS bytes as a Lua string. |
| [`text.to_shift_jis(text)`](#textto_shift_jis) | A Lua string as Shift-JIS bytes. |
| [`text.plain(text)`](#textplain) | A line with its auto-translate phrases turned into plain words. |
| [`text.strip(text)`](#textstrip) | A line without its in-line codes. |
| [`text.colour(text, code)`](#textcolour) | Text in one of the log's in-line colours. |
| [`text.phrase(group, id)`](#textphrase) | The phrase with this group and id, ready to put in chat text. |
| [`text.autotranslate(text)`](#textautotranslate) | The phrase whose text is exactly this, ready to put in chat text. |

## `text.from_shift_jis`

```lua
text.from_shift_jis(bytes) -> string
```

Decodes Shift-JIS bytes, such as a name or a chat line read from a packet or a game file, into a Lua string.
Auto-translate phrases and the game's in-line colour codes in the bytes become special characters, so
`text.to_shift_jis` gives the same bytes back.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `bytes` | `string` | yes | The raw bytes. |

**Returns** `string`: the text. A single byte that is not valid Shift-JIS becomes U+FFFD (the replacement character);
a two-byte code that standard Shift-JIS lacks (one of the game's glyphs) is kept as a special character, so it
encodes back to the same bytes.

**Example**

```lua
local bytes = text.to_shift_jis("Hello")
print(#bytes, text.from_shift_jis(bytes))
```

The bytes are decoded up to their end: cut a fixed-size field at its first zero byte yourself, for example with
`bytes:match("^[^\0]*")`.

## `text.to_shift_jis`

```lua
text.to_shift_jis(text) -> string
```

Encodes a Lua string as Shift-JIS bytes, for a packet or a file in the game's format. Phrases and colour codes made
by `text.from_shift_jis`, `text.phrase` or `text.autotranslate` turn back into their bytes.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `text` | `string` | yes | The text (valid UTF-8). |

**Returns** `string`: the bytes. A character Shift-JIS cannot hold becomes `?`.

**Errors** A `text` that is not valid UTF-8 raises an error.

**Example**

```lua
local bytes = text.to_shift_jis("Café")
print(#bytes .. " bytes")
```

## `text.plain`

```lua
text.plain(text) -> string
```

Returns the line with each auto-translate phrase replaced by its words (without the phrase's brackets), for matching or
logging chat text.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `text` | `string` | yes | A line that may hold phrases. |

**Returns** `string`: the plain line. While the dictionary has not loaded, the phrases are removed instead.

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
events.on("chat_message", function(line, sender, kind)
  if kind == 3 then
    log.info("tell from " .. sender .. ": " .. text.plain(line))
  end
end)
```

## `text.strip`

```lua
text.strip(text) -> string
```

The line without its in-line codes: the colour and indent codes the game's text carries (and `text.colour` adds) are
dropped, the words stay.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `text` | `string` | yes | A line, as a battle filter or the log has it. |

**Returns** `string`.

**Example**

```lua
print(text.strip(text.colour("Wild Rabbit", 8)))  --> Wild Rabbit
```

## `text.colour`

```lua
text.colour(text, code) -> string
```

`text` in one of the log's in-line colours, then back to the line's colour, as the game colours names and items
in its lines. Put it in a battle filter's lines, `chat.add` text, or a chat filter's rewrite.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `text` | `string` | yes | The text to colour. |
| `code` | `integer` | yes | The colour code: 7 ours, 6 party, 3 alliance, 8 a monster our party claimed, 5 another's claim (Battle Names' colours), 2 an item's; 0 leaves the text as it is. |

**Returns** `string`.

**Example**

```lua
chat.add("Watch out for the " .. text.colour("Wild Rabbit", 8) .. "!")
```

## `text.phrase`

```lua
text.phrase(group, id) -> string
```

Builds the phrase with this group and phrase id as a chat line holds it. Put it in text you send or show; the chat log
shows it as the phrase. The ids come from [`res.autotranslate`](res.md#resautotranslate) (whose entries also carry the
same string as `phrase`).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `group` | `integer` | yes | The group id, 0..255. |
| `id` | `integer` | yes | The phrase id within the group, 0..255 (0 is the group's title). |

**Returns** `string`: the phrase. It is built from the two numbers whether or not the dictionary has such a phrase.

**Errors** A `group` or `id` outside 0..255 raises an error.

**Example**

```lua
local hits = res.autotranslate("thank")
local e = hits and hits[1]
if e and e.id ~= 0 then
  chat.add("Say it with " .. text.phrase(e.group, e.id))
end
```

## `text.autotranslate`

```lua
text.autotranslate(text) -> string?
```

Finds the phrase whose text is exactly `text` (ignoring case and the spaces around it) and returns it as a chat line
holds it. A group's title also matches, without its brackets, and gives that group's title phrase.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `text` | `string` | yes | The phrase's exact words. |

**Returns** `string?`: the phrase, or `nil` when the dictionary has no such phrase or has not loaded yet.

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
commands.register("at", function(...)
  local p = text.autotranslate(table.concat({ ... }, " "))
  if p then
    chat.add(p)
  else
    print("No such phrase.")
  end
end)
```

Only the English dictionary is loaded.
