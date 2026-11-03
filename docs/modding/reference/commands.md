# `commands`

The mod's own text commands, and running lines as if the player typed them. `commands.register` adds a command the
player types in the chat line as `//name` or `/name` (in a macro or a key bind too); `commands.run` runs any line, a
game command or a mod's. Commands are read in the world only: in the lobby, nothing typed reaches a mod. Registering
needs no permission; running a game command does (see [commands.run](#commandsrun)).

```lua
commands.register("where", function()
  local p = game.player()
  print(("x %.1f  z %.1f  in %s"):format(p.pos.x, p.pos.z, res.zone(p.zone) or p.zone))
end)
-- The player types //where (or /where).
```

## Functions

| Function | What it does |
| --- | --- |
| [`commands.register(name, handler)`](#commandsregister) | Adds a command `//name` / `/name` that calls `handler` with the words typed after it. |
| [`commands.run(line)`](#commandsrun) | Runs a line as if the player had typed it. |

## `commands.register`

```lua
commands.register(name, handler)
```

Adds a text command. When the player types `//name` or `/name` followed by words, `handler` is called with each word
as a separate string argument.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | yes | The command's name. Leading slashes are dropped and case does not matter (`"Hello"`, `"/hello"` and `"hello"` are the same command). It must not be empty, contain spaces, be `lua` or be the name of one of the game's commands (`sit`, `ma`, `equip`, ...). |
| `handler` | `function` | yes | Called as `handler(word1, word2, ...)` with the words after the name, all strings. With no words it gets no arguments. |

**Returns** nothing.

**Errors** `command name "<name>"` when the name is empty or contains whitespace; `/<name> is a game command: give yours another name` for `lua` or a game command's name.

**Example**

```lua
commands.register("hello", function(...)
  local words = { ... }
  if #words == 0 then
    print("Usage: //hello <name>")
    return
  end
  print("Hello, " .. table.concat(words, " ") .. "!")
end)
```

How a typed line reaches the mod:

- A line starting with `//` always goes to the mods. If no running mod has that command, the log shows
  "No mod command <name>." in red.
- A line starting with one `/` goes to the mods when its first word is a command a running mod registered (or
  `/lua`). Since a mod cannot take a game command's name, the game's commands always work as before.
- The words are split at spaces. Quotes do not group words: `//note "a b"` gives `"a` and `b"`. Join them again with
  `table.concat({ ... }, " ")`.
- Registering a name again replaces the handler. When two mods register the same name, the one loaded first gets
  the command.
- A suspended mod's commands are not taken: `/name` then goes to the game's commands.
- The handler runs in the next mod phase, as a coroutine (it may sleep). When the player typed the line (or a macro or
  a key bind ran it), the handler counts as the player's input: game actions in it need only the `actions`
  permission, also after a sleep. A command run by a mod with `commands.run` is not the player's input.

## `commands.run`

```lua
commands.run(line)
```

Runs a line as if the player had typed it in the chat line: a game command (`/ma "Cure" <me>`), a client command, a
mod's command (`//name`), or plain text, which is said in the current chat mode.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `line` | `string` | yes | The line, e.g. `'/ma "Cure" <me>'` or `"//hello world"`. |

**Returns** nothing.

**Needs** a line that does not start with `//` is a game action: it needs the `actions` permission while handling the
player's input (a command they typed, a window or HUD click), or the `automation` permission any time. A line starting
with `//` needs no permission.

**Errors**

- "game actions need the actions or automation permission: list it in mod.toml, then /lua grant <mod> actions" when
  the mod has neither.
- "a game action outside the player's input needs the automation permission" when the mod has `actions` but the call
  comes from an event handler, a timer or another place that is not the player's input.

**Example**

```lua
-- mod.toml lists: permissions = ["actions"]
commands.register("cureme", function()
  commands.run('/ma "Cure" <me>')
end)
```

The line is queued and runs after the mod phase. A line for a mod (`//name`, `/lua`, or a mod's `/name`) runs in the
next mod phase. Any other line goes to the game's command handling just as if typed: it passes every mod's typed-line
filters (see [filters.typed](filters.md#filterstyped)), and refusals print in the log as they would for the player.
