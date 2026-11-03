# `fs`

`fs` reads and writes files in the mod's data folder: `mod-data/<mod id>/files` in the client's data folder (on Linux
`~/.local/share/vana-client/mod-data/<mod id>/files`). Every write goes there. Reads (`fs.read`, `fs.exists`,
`fs.list`) also see the files that came with the mod, in its own folder, as a read-only fallback: the data folder
first, then the mod's own files, so a mod can ship default data and save the user's copy over it. A path whose first
part is `shared` (`"shared/notes.txt"`) is in the shared area instead, a folder every mod with the `files.shared`
permission can read and write. The calls work anywhere (also in the lobby and in filters) and take effect at once.

```lua
local raw = fs.read("config.json")          -- the saved copy, else the one shipped with the mod
local cfg = raw and json.decode(raw) or {}
cfg.runs = (cfg.runs or 0) + 1
fs.write("config.json", json.encode(cfg))   -- always saved in the data folder
```

Paths are relative and use `/` (a `\` works too). A leading `/` and `.` parts are ignored, so `"/a/./b.txt"` is
`"a/b.txt"`. A path with a `..` part raises "outside the mod's folder", and one with a `:` in a part raises "not a
relative path". Errors from the system come as "<path>: <reason>".

## Functions

| Function | What it does |
| --- | --- |
| [`fs.read(path)`](#fsread) | A file's contents, or nil. |
| [`fs.write(path, data)`](#fswrite) | Writes a file, replacing what was there. |
| [`fs.append(path, data)`](#fsappend) | Adds to the end of a file. |
| [`fs.exists(path)`](#fsexists) | Whether a file or folder is there. |
| [`fs.list(path?)`](#fslist) | The names in a folder. |
| [`fs.mkdir(path)`](#fsmkdir) | Makes a folder. |
| [`fs.remove(path)`](#fsremove) | Removes a file, or a folder with everything in it. |
| [`fs.rename(from, to)`](#fsrename) | Moves or renames a file or folder. |

## `fs.read`

```lua
fs.read(path) -> string?
```

Reads a whole file. It looks in the data folder (or the shared area) first; when the file is not there, it reads the
mod's own file at the same path.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `path` | `string` | yes | The file's relative path. |

**Returns** `string?`: the file's contents (any bytes), or `nil` when neither place has the file.

**Needs** the `files.shared` permission for a path under `shared/`.

**Errors** A refused path (see above); "this needs the files.shared permission: ..." for `shared/` without the
permission; "<path>: <reason>" when the file is there but cannot be read (a folder, for example).

**Example**

```lua
local notes = fs.read("notes.txt")
if notes then
  for line in notes:gmatch("[^\n]+") do print(line) end
else
  print("No notes yet.")
end
```

A file from the mod's own folder is read as text: one there that is not valid UTF-8 reads as `nil`. Binary files in
the data folder read as they are.

## `fs.write`

```lua
fs.write(path, data)
```

Writes `data` to a file in the data folder (or the shared area), replacing the file if it exists. Missing folders on
the way are made.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `path` | `string` | yes | The file's relative path. |
| `data` | `string` | yes | What to write (any bytes). |

**Returns** nothing.

**Needs** the `files.shared` permission for a path under `shared/`.

**Errors** A refused path; "this needs the files.shared permission: ..." for `shared/` without the permission;
"<path>: <reason>" when the system refuses the write.

**Example**

```lua
local p = game.player()
fs.write("last_zone.txt", tostring(p.zone))
fs.write("logs/" .. os.date("%Y-%m-%d") .. ".txt", "started\n")
```

## `fs.append`

```lua
fs.append(path, data)
```

Adds `data` to the end of a file in the data folder (or the shared area), making the file and its folders when they
are missing. Good for logs.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `path` | `string` | yes | The file's relative path. |
| `data` | `string` | yes | What to add (any bytes). |

**Returns** nothing.

**Needs** the `files.shared` permission for a path under `shared/`.

**Errors** As [`fs.write`](#fswrite).

**Example**

```lua
events.on("chat_message", function(line, sender)
  fs.append("chat.log", os.date("%H:%M ") .. sender .. ": " .. text.plain(line) .. "\n")
end)
```

## `fs.exists`

```lua
fs.exists(path) -> boolean
```

Whether a file or folder with this path is in the data folder (or the shared area) or among the mod's own files.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `path` | `string` | yes | The relative path. |

**Returns** `boolean`: `true` when either place has it.

**Needs** the `files.shared` permission for a path under `shared/`.

**Errors** A refused path; "this needs the files.shared permission: ..." for `shared/` without the permission.

**Example**

```lua
if not fs.exists("data") then
  fs.mkdir("data")
end
```

## `fs.list`

```lua
fs.list(path?) -> string[]
```

The names in a folder, sorted, with a `/` at the end of each folder's name. The data folder's names and the mod's own
are merged (each name once). For a path under `shared/`, only the shared area is listed.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `path` | `string` | no, default `""` | The folder's relative path; none lists the top. |

**Returns** `string[]`: the names (not paths). A folder that does not exist gives an empty list.

**Needs** the `files.shared` permission for a path under `shared/`.

**Errors** A refused path; "this needs the files.shared permission: ..." for `shared/` without the permission.

**Example**

```lua
for _, name in ipairs(fs.list("logs")) do
  if name:sub(-1) ~= "/" then
    print(name)
  end
end
```

## `fs.mkdir`

```lua
fs.mkdir(path)
```

Makes a folder in the data folder (or the shared area), with any missing folders above it. A folder that already
exists is not an error.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `path` | `string` | yes | The folder's relative path. |

**Returns** nothing.

**Needs** the `files.shared` permission for a path under `shared/`.

**Errors** A refused path; "this needs the files.shared permission: ..." for `shared/` without the permission;
"<path>: <reason>" when the system refuses.

**Example**

```lua
fs.mkdir("cache/maps")
fs.write("cache/maps/readme.txt", "made by " .. mod.name)
```

## `fs.remove`

```lua
fs.remove(path)
```

Removes a file, or a folder with everything in it, from the data folder (or the shared area). Nothing there is not
an error. The mod's own files are never removed.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `path` | `string` | yes | The relative path. |

**Returns** nothing.

**Needs** the `files.shared` permission for a path under `shared/`.

**Errors** A refused path; "fs.remove \"\": name a file or folder inside the data folder" for `""`, `"/"`, `"."` or
`"shared"` (the whole data folder or the whole shared area); "this needs the files.shared permission: ..." for
`shared/` without the permission; "<path>: <reason>" when the system refuses.

**Example**

```lua
for _, name in ipairs(fs.list("cache")) do
  fs.remove("cache/" .. name)
end
```

## `fs.rename`

```lua
fs.rename(from, to)
```

Moves or renames a file or folder within the data folder (or between it and the shared area). Missing folders above
`to` are made; a file already at `to` is replaced.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `from` | `string` | yes | The relative path of what to move. |
| `to` | `string` | yes | Its new relative path. |

**Returns** nothing.

**Needs** the `files.shared` permission when either path is under `shared/`.

**Errors** A refused path; "this needs the files.shared permission: ..." for `shared/` without the permission;
"<from>: <reason>" when the move fails (for example when `from` does not exist).

**Example**

```lua
local tmp = "save.json.tmp"
fs.write(tmp, json.encode(storage.account))
fs.rename(tmp, "save.json")
```

Only files in the data folder can be moved: to copy one of the mod's own files there, `fs.write` what `fs.read`
returns.
