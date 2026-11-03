# `lobby`

The screens before login: the login form, the title and character select screens. `lobby.state` tells a mod which
screen shows and what is on it; `lobby.sign_in` and `lobby.play` act on those screens as the player's buttons do, for
an automatic sign-in or character pick. Only a lobby mod (one with `lobby = true` in its mod.toml) runs there, so
these calls are for lobby mods. Acting needs the `automation` permission.

```lua
-- Log in with the character named in the mod's settings, once the select screen is up.
events.on("lobby_screen", function(screen)
  if screen == "title" or screen == "select" then
    wait_until(function() return not lobby.state().busy end, 10)
    lobby.play(settings.get("character"))
  end
end)
```

## Lobby mods

A mod runs in the lobby when its mod.toml says so:

```toml
id = "autologin"
name = "Auto Login"
lobby = true
permissions = ["automation"]

[settings.character]
type = "string"
default = ""
label = "Character"
```

Its life differs from other mods':

- It loads when the client first shows the lobby (if it is enabled in the mod list), and gets `load` and then
  `lobby_screen` events as the screens change.
- In the lobby there is no character: reading game state (`game.*`, `chat.line` and the like) raises "game state is
  not available here (in the lobby, or outside the mod phase)". `storage.account` is there; `storage.character` is an
  empty table, and character-scoped settings have their defaults.
- What runs in the lobby: event handlers (`load`, `lobby_screen`, `frame`, events from `mods.emit`), timers, sleeps
  and waits, and network callbacks. Typed commands do not reach mods there, and the HUD, windows, sounds and game
  actions only work in the world. `print` and `chat.add` lines go to vana-client.log. `input.press` presses keys on
  the lobby's screens.
- When a character enters the world, the lobby mod keeps running: it gets that character's `storage.character` and
  settings, then `login` and `lobby_screen` with `"world"`, and from then on everything the other mods get.
- At logout it gets `logout` but is not unloaded: its `storage.character` is saved and emptied, and it goes on in the
  lobby (`lobby_screen` with `"login"`).
- It is unloaded when the client closes (`unload`, without game state) or with `/lua unload`.

## Functions

| Function | What it does |
| --- | --- |
| [`lobby.state()`](#lobbystate) | The lobby screen showing and what is on it. |
| [`lobby.sign_in(login)`](#lobbysign_in) | Signs in from the login screen, as its Log in button does. |
| [`lobby.play(character)`](#lobbyplay) | Enters the world with a character, as the select screen's Play does. |

## `lobby.state`

```lua
lobby.state() -> table
```

The lobby as it shows now. Works in the lobby and in the world (there `screen` is `"world"` and the other fields keep
their last lobby values).

**Returns** `table`:

| Field | Type | Description |
| --- | --- | --- |
| `screen` | `string` | `login` (the login form), `connecting` (a character was picked and the world is loading), `title`, `select`, `create`, `delete`, `config`, or `world`. |
| `server` | `string` | The server the client signs in to. |
| `account` | `string` | The account name. |
| `playonline` | `boolean` | Whether the server is a PlayOnline one. |
| `busy` | `boolean` | A fade is running, a reply is awaited or an error shows: `lobby.sign_in` and `lobby.play` are dropped meanwhile. |
| `error` | `string?` | The error the login screen shows, or nil. |
| `selected` | `integer?` | The select screen's character, by slot (from 1); nil for none. |
| `characters` | `table[]` | The account's character slots, see below; empty before the character list arrives. |

**`characters` entries**

| Field | Type | Description |
| --- | --- | --- |
| `slot` | `integer` | The slot, from 1 (what `lobby.play` takes). |
| `name` | `string` | The character's name. |
| `world` | `string` | The world (server) it lives on. |
| `race` | `integer` | Race id. |
| `job` | `integer` | Main job id. |
| `level` | `integer` | Main job level. |
| `sub_job` | `integer` | Support job id. |
| `zone` | `integer` | The zone it is in. |
| `available` | `boolean` | Whether it can be played: false for an empty slot, or a character the server does not offer for play. `lobby.play` drops a character that is not available. |

**Example**

```lua
local s = lobby.state()
for _, c in ipairs(s.characters) do
  if c.available then
    log.info(c.slot .. ": " .. c.name .. " (level " .. c.level .. ")")
  end
end
```

## `lobby.sign_in`

```lua
lobby.sign_in(login)
```

Signs in from the login screen with the account and password given, as the login form's Log in button does.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `login` | `table` | yes | The sign-in, see below. |

**`login` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `account` | `string` | `""` | The account name. |
| `password` | `string` | `""` | The password. |
| `otp` | `string` | `""` | The two-factor code, for an account that uses one. |
| `server` | `string` | the configured server | The server to sign in to. |
| `pol_id` | `string` | the configured one | A PlayOnline server's PlayOnline ID. |
| `pol_password` | `string` | `""` | A PlayOnline server's PlayOnline password. |

**Returns** nothing.

**Needs** the `automation` permission. (The `actions` permission works only while handling the player's input, which
never happens in the lobby.)

**Errors** "game actions need the actions or automation permission: list it in mod.toml, then /lua grant <mod> actions"
or "a game action outside the player's input needs the automation permission" without it.

**Example**

```lua
-- The password is kept sealed: storage.seal(password) made it once.
events.on("lobby_screen", function(screen)
  if screen ~= "login" then return end
  local password = storage.unseal(storage.account.sealed or "")
  if password then
    lobby.sign_in({ account = storage.account.login, password = password })
  end
end)
```

The request is queued and taken after the mod phase. It is used only on the login screen while the lobby is not busy;
anywhere else it is dropped without an error. A failed sign-in shows on the login screen as usual (`lobby.state().error`).

## `lobby.play`

```lua
lobby.play(character)
```

Enters the world with a character, as the select screen's Play does.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `character` | `string` or `integer` | yes | The character's name (case does not matter), or its slot from 1 (as `lobby.state().characters` gives it). |

**Returns** nothing.

**Needs** the `automation` permission, as `lobby.sign_in`.

**Errors** "lobby.play takes a character's name or its slot (from 1)" for anything but a string or an integer from
1; the permission errors as `lobby.sign_in`.

**Example**

```lua
events.on("lobby_screen", function(screen)
  if screen == "select" then
    lobby.play(1)   -- the first slot
  end
end)
```

The request is queued and taken after the mod phase. It is used on the character screens (`title`, `select`,
`create`, `delete`, `config`) while the lobby is not busy, and only for an available character; otherwise it is
dropped without an error.
