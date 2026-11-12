# `filters`

Filters see lines before the client takes them, and keep, change or drop them: `filters.chat` sees every line about to
go into the chat log, `filters.typed` every line about to run as a command, `filters.battle` every battle message with
its data before it prints. A filter is a plain function that returns
its verdict. Filters run inline, inside the client's work, not in the mod phase: they cannot sleep, and each call
has a budget of about 20 ms. Chat filters cannot read game state; typed and battle filters can. No permission is needed. A
suspended mod's filters stop until it is reloaded.

```lua
-- Hide every battle line about a miss.
filters.chat(function(text, mode, battle)
  if battle and text:find("misses", 1, true) then
    return false
  end
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`filters.chat(filter)`](#filterschat) | Runs `filter` on every chat log line before it is kept: keep, rewrite or drop it. |
| [`filters.typed(filter)`](#filterstyped) | Runs `filter` on every typed or run line before it is handled: keep, rewrite or drop it. |
| [`filters.battle(filter)`](#filtersbattle) | Runs `filter` on every battle message with its data: colour its names, rewrite or drop it. |
| [`filters.off(id)`](#filtersoff) | Removes a filter. |

## `filters.chat`

```lua
filters.chat(filter) -> integer
```

Adds a chat filter: `filter` is called for every line about to be added to the chat log, and its return value says
what happens to the line.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `filter` | `function` | yes | Called as `filter(text, mode, battle)`; see below. |

**`filter` arguments**

| Argument | Type | Description |
| --- | --- | --- |
| `text` | `string` | The line as it would be stored. It can hold the game's in-line colour codes and auto-translate phrases as special characters, so search it with plain `string.find(text, word, 1, true)`. |
| `mode` | `integer?` | The line's chat mode (0..255), which sets its colour and which log windows show it; nil for a line printed without one (the client's notices and many system lines). |
| `battle` | `boolean` | Whether the line is a battle message. |

**What `filter` returns**

| Return | What happens |
| --- | --- |
| nothing, `nil` or `true` | The line is kept as it is (as earlier filters left it). |
| `false` | The line is dropped. Later filters do not see it. |
| a string | The line's text is replaced by it. |
| a string and an integer | The text is replaced and the line moves to that mode (0..255): it takes the mode's colour. A battle line stays a battle line. |

**Returns** `integer`: the filter's id, for `filters.off`.

**Example**

```lua
-- Mark received tells (the client prints them in mode 12).
filters.chat(function(text, mode)
  if mode == 12 then
    return "[TELL] " .. text
  end
end)
```

Notes:

- Every line goes through the filters: players' chat, battle messages, system and event text, the client's notices,
  and lines mods add with `print` and `chat.add` (this mod's too: a line you `print` from a chat filter comes back
  through it).
- With several filters, mods run in load order and each mod's filters in the order added. Each one sees the text and
  mode as the one before left them.
- A new or removed chat filter takes effect at the end of the mod phase in which `filters.chat` or `filters.off` was
  called.
- A filter cannot sleep: `sleep` raises "attempt to yield from outside a coroutine". Reading game state (`game.*`,
  `chat.line` and the like) raises "game state is not available here (in the lobby, or outside the mod phase)".
- A call that runs past its budget is stopped with "ran past its time budget". An error in a filter leaves the line as
  it was before that filter, is printed in the log (once per distinct message) and counts toward the mod's suspension
  (20 errors within 60 seconds).

## `filters.typed`

```lua
filters.typed(filter) -> integer
```

Adds a typed-line filter: `filter` is called for every line about to run as a command or be said in chat, and its
return value says what happens to the line.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `filter` | `function` | yes | Called as `filter(line)` with the line as a string, spaces trimmed from both ends. |

**What `filter` returns**

| Return | What happens |
| --- | --- |
| nothing, `nil` or `true` | The line runs as it is (as earlier filters left it). |
| `false` | The line is dropped: nothing runs. Later filters do not see it. |
| a string | The line is replaced by it (trimmed; an empty string runs nothing). |

**Returns** `integer`: the filter's id, for `filters.off`.

**Example**

```lua
-- Typing /hi greets the target.
filters.typed(function(line)
  if line == "/hi" then
    return "/say Hello there, <t>!"
  end
end)
```

Notes:

- Typed filters see lines typed in the chat line, macro lines, key bind lines, lines mods run with `commands.run`,
  and the lines the game's menus run (a spell picked in the magic menu runs `/magic "Cure" <t>`). They do not see
  what mods' `actions` calls run, so one mod cannot rewrite another's action. They do not see lines for mods (`//name`, `/lua`, or a mod's registered `/name`): those go to the mods first. A line
  finished in the sub-target prompt (`<st>`) is not filtered a second time.
- The line comes as typed: text without a leading `/` arrives without the chat mode's prefix (`hello` typed in say
  mode arrives as `hello`).
- A replacement goes on to the game's command handling. It is not offered to mod commands again, so returning
  `"//name"` does not run a mod command.
- Mods run in load order and each mod's filters in the order added; each sees the line as the one before left it.
  A new filter takes effect at once.
- Typed filters can read game state (`game.player()` and the like). They cannot sleep and have the same 20 ms budget
  as chat filters; errors are reported and counted the same way.

## `filters.battle`

```lua
filters.battle(filter) -> integer
```

Adds a battle message filter: `filter` is called for each battle message the log is about to print (an action's
lines, s2c 0x029 and 0x02D messages), after the game's Chat Filters have let it through, with what the message is
about. It can colour the names in it, replace its lines or drop it. The bundled Battle Text mod is written with it.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `filter` | `function` | yes | Called as `filter(m)` with the message's table. |

**`m` fields**

| Field | Type | Description |
| --- | --- | --- |
| `id` | `integer` | The battle message's id. |
| `category` | `integer?` | The 0x028 action's category (7 a weapon skill starts, 8 a spell, 9 an item, 12 a ranged attack, ...); `nil` for 0x029 and 0x02D messages. |
| `value` | `integer` | The record's value (the damage, the HP healed, the status id). |
| `mode` | `integer` | The chat mode the lines print in. |
| `lines` | `string[]` | The lines as printed so far, in-line codes kept ([`text.strip`](text.md#textstrip) gives the words). |
| `actor` | `table?` | The actor: `id`; and where it is known, `name`, `pc` (a player: no "the"), `named` (a proper noun: no "the"), `colour`. |
| `target` | `table?` | The target, the same fields. |

**What `filter` returns**

| Return | What happens |
| --- | --- |
| nothing or `nil` | The lines print as they are. Colours set on `actor.colour` / `target.colour` draw those names in that colour code (see [`text.colour`](text.md#textcolour)). |
| `false` | The message is dropped. Later filters do not see it. |
| a string, or a list of strings | Its lines are replaced, in the message's colour. Replaced lines are not condensed by the modern chat. |

**Returns** `integer`: the filter's id, for `filters.off`.

**Example**

```lua
-- Our name in the party colour, and every miss dropped.
filters.battle(function(m)
  if m.actor and m.actor.id == game.player().id then m.actor.colour = 6 end
  for _, line in ipairs(m.lines) do
    if text.strip(line):find(" misses ", 1, true) then return false end
  end
end)
```

Notes:

- Mods run in load order and each mod's filters in the order added; each sees the lines and colours the one before
  left.
- Battle filters can read game state; they cannot sleep and have the same 20 ms budget as chat filters.
- The lines a battle filter keeps or writes still pass the chat filters on their way into the log.

## `filters.off`

```lua
filters.off(id)
```

Removes a chat, typed or battle filter. An id that matches none does nothing.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | What `filters.chat`, `filters.typed` or `filters.battle` returned. |

**Returns** nothing.

**Example**

```lua
local quiet
commands.register("quiet", function()
  if quiet then
    filters.off(quiet)
    quiet = nil
  else
    quiet = filters.chat(function(text, mode, battle) if battle then return false end end)
  end
end)
```
