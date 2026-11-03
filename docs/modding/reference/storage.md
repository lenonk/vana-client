# `storage`

`storage` keeps a mod's data between sessions. It holds two plain tables the client saves for you, and functions to
save them now and to seal secrets. Read and change the tables like any Lua table; the client writes them to disk as
JSON at logout, when the mod unloads (`/lua unload`, `/lua reload`, the client closing) and when you call
`storage.save()`. The functions work anywhere (also in the lobby) and need no permission.

- **`storage.account`**: one table for the mod, shared by every character you play on this computer. It is loaded
  when the mod loads.
- **`storage.character`**: one table per character (by world and character name), loaded when the mod loads at
  login. A `lobby` mod runs before login: its `storage.character` is an empty table until a character logs in, is
  replaced by that character's table at login, and is saved and replaced by an empty table again at logout. Read
  `storage.character` afresh each time rather than keeping the table in a variable.

```lua
events.on("level_up", function(level)
  storage.character.best = math.max(storage.character.best or 0, level)
  storage.account.level_ups = (storage.account.level_ups or 0) + 1
  storage.save()
end)
```

What can be stored: booleans, numbers, strings and tables of them, as for [`json.encode`](json.md#jsonencode).

- A list (keys 1 to n with no gaps, as in `{a, b, c}`) is saved as a list and comes back as one.
- Every other key is saved as text: `t[4096]` comes back as `t["4096"]`, `t[true]` as `t["true"]`. Use string keys
  (`t[tostring(id)]`) for anything indexed by an id, so the same key works before and after a reload.
- Values that are not data (functions, coroutines, userdata) are left out; a line in the chat log names each one, and
  the rest is saved.
- A number that is not finite (`math.huge`, `0/0`), a string that is not UTF-8 text, or tables nested more than 64
  deep (a table inside itself) stop that table (`account` or `character`) from being saved; a line in the chat log
  says why, and the file on disk keeps its last good contents.
- Keep the top level of each table keyed by strings: a top-level list is saved but not loaded back.
- You may replace a table (`storage.account = {}`): the client saves whatever `storage.account` and
  `storage.character` hold at the time.

The files are in `mod-data/<mod id>/` in the client's data folder: `account.json` and
`character-<world>-<name>.json`.

## Functions

| Function | What it does |
| --- | --- |
| [`storage.save()`](#storagesave) | Writes both tables to disk at the end of this frame. |
| [`storage.seal(text)`](#storageseal) | Encrypts a secret so only this mod on this computer can read it. |
| [`storage.unseal(sealed)`](#storageunseal) | Decrypts what `storage.seal` made. |

## `storage.save`

```lua
storage.save()
```

Asks for `storage.account` and `storage.character` to be written now, rather than waiting for logout or unload. Call
it after a change you cannot afford to lose if the client stops unexpectedly.

**Returns** nothing.

**Example**

```lua
commands.register("note", function(...)
  local list = storage.character.notes or {}
  list[#list + 1] = table.concat({ ... }, " ")
  storage.character.notes = list
  storage.save()
end)
```

The files are written at the end of the current mod phase (this frame), once however many times you call it. In the
lobby only `storage.account` is written, since no character is logged in.

## `storage.seal`

```lua
storage.seal(text) -> string
```

Encrypts `text` (a password, a token) with a secret key kept on this computer, bound to this mod. Store the result in
`storage` or a file; only this mod, on this computer, can open it again with `storage.unseal`.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `text` | `string` | yes | The secret. |

**Returns** `string`: the sealed text, in hexadecimal digits. Sealing the same text twice gives different results;
both open.

**Errors** An error with the reason when the computer's key cannot be read or made.

**Example**

```lua
commands.register("settoken", function(token)
  storage.account.token = storage.seal(token or "")
  storage.save()
  print("Token saved.")
end)
```

## `storage.unseal`

```lua
storage.unseal(sealed) -> string?
```

Opens text that `storage.seal` made.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `sealed` | `string` | yes | What `storage.seal` returned. |

**Returns** `string?`: the original text, or `nil` when it does not open: sealed by another mod or on another
computer, changed or damaged, or empty.

**Example**

```lua
local token = storage.unseal(storage.account.token or "")
if not token then
  print("No token: set one with //settoken <token>")
end
```
