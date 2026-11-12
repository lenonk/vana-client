# `commands`

The mod's text commands, and running lines as if the player typed them. `commands.register` adds a command the
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
| [`commands.is_game(name)`](#commandsis_game) | Whether `/name` is one of the game's text commands. |
| [`commands.usage(name)`](#commandsusage) | The command's forms as your mod.toml declares them, as lines to print. |
| [`commands.run(line)`](#commandsrun) | Runs a line as if the player had typed it. |

## `commands.register`

```lua
commands.register(name, handler)
```

Adds a text command. When the player types `//name` or `/name` followed by words, `handler` is called with each word
as a separate string argument. Declare it in mod.toml (`[commands.<name>]`, with `help` and `usage`) so the chat line's
hint, `//mods` and the Mods page show what it takes; see the guide's "Commands and typed lines".

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
  "No mod command <name>." in red, with the command it most likely meant. `//mods`
  lists the running mods' commands (a mod cannot register `mods`).
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

## `commands.usage`

```lua
commands.usage(name) -> string[]
```

The forms of one of your commands as your mod.toml declares them (`[commands.<name>]`: its `args`, then each of its
`forms`), each as the line a player would type. Print them when a command is used wrongly, so the usage you show is the
one players see in the chat line's suggestions and in `/lua help`.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | yes | The command's name, without the slash. |

**Returns** `string[]`: one line a form, such as `"//trusts <lineup>"`; empty when your mod.toml declares no such
command.

**Example**

```lua
commands.register("trusts", function(cmd, ...)
  if not cmd then
    for _, line in ipairs(commands.usage("trusts")) do chat.add(line) end
    return
  end
  -- ...
end)
```

## `commands.is_game`

```lua
commands.is_game(name) -> boolean
```

Whether `/name` is one of the game's text commands (`/ma`, `/tell`, `/away`...). A mod's command may not take such a
name; a filter on typed lines can use this to leave the game's commands alone.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | yes | The command, with or without the slash. |

**Returns** `boolean`.

**Example**

```lua
filters.typed(function(line)
  local word = line:match("^/(%S+)")
  if word and not commands.is_game(word) then
    -- a word of ours to read
  end
end)
```

## `commands.run`

```lua
commands.run(line)
```

Runs a line as if the player had typed it in the chat line: a game command (`/ma "Cure" <me>`), a client command, a
mod's command (`//name`), or plain text, which is said in the current chat mode. A mod's command run this way is the
player's input when your mod runs it on the player's input (a click, a key bind): its game actions need only the
`actions` permission then, as when the player types it.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `line` | `string` | yes | The line, e.g. `'/ma "Cure" <me>'` or `"//hello world"`. |

**Returns** nothing.

**Needs** a line that does not start with `//` is a game action: it needs the `actions` permission while handling the
player's input (a command they typed, a window or HUD click), or the `automation` permission any time. A line starting
with `//` needs no permission, except `/lua load`, `unload`, `reload`, `hide` and `show` (with any number of slashes),
which control the other mods and need the `mods` permission.

**Errors**

- "game actions need the actions or automation permission: list it in mod.toml, then /lua grant <mod> actions" when
  the mod has neither.
- "a game action outside the player's input needs the automation permission" when the mod has `actions` but the call
  comes from an event handler, a timer or another place that is not the player's input.
- "this needs the mods permission: list it in mod.toml, then /lua grant <mod> mods" for `/lua load`, `unload`,
  `reload`, `hide` or `show` without it.

**Example**

```lua
-- mod.toml lists: permissions = ["actions"]
commands.register("cureme", function()
  commands.run('/ma "Cure" <me>')
end)
```

The line is queued and runs after the mod phase. A line for a mod (`//name`, `/lua`, or a mod's `/name`) runs in the
next mod phase, as the mod's line and not the player's (leading spaces are trimmed first): `/lua grant`, `revoke`,
`run`, `set` and `watch` are refused there, as they are from a mod's bind, a macro a mod started, or a chat line a mod
submitted with `input.press("enter")`. `/lua load`, `unload`, `reload`, `hide` and `show` run only for a mod with the
`mods` permission (from `commands.run` or its bind), and never from a macro a mod started or a chat line it submitted. Any other line goes to the game's command handling just as if typed: it passes every mod's typed-line
filters (see [filters.typed](filters.md#filterstyped)), and refusals print in the log as they would for the player.
