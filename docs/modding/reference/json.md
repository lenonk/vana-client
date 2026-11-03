# `json`

`json` turns plain Lua data into JSON text and back: for files (`fs`), web requests (`net`) and data shared with other
programs. Both functions work anywhere (also in the lobby and in filters) and need no permission. Plain data means
`nil`, booleans, numbers, strings and tables of them; a table with a length (`#t > 0`) is a JSON array, any other
table a JSON object.

```lua
local data = { name = "Goblin", seen = { 12, 30 }, killed = true }
local s = json.encode(data)
local back = json.decode(s)
print(s, back.seen[2])
```

## Functions

| Function | What it does |
| --- | --- |
| [`json.encode(value, options?)`](#jsonencode) | Plain Lua data as JSON text. |
| [`json.decode(text)`](#jsondecode) | JSON text as Lua data. |

## `json.encode`

```lua
json.encode(value, options?) -> string
```

Writes a Lua value as JSON text.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `value` | `any` | yes | Plain data: `nil`, a boolean, a number, a string or a table of them. |
| `options` | `table` | no | How to write it. |

**`options` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `pretty` | `boolean` | `false` | `true` writes indented JSON over several lines. |

**Returns** `string`: the JSON text.

**Errors** "json.encode: <where> is not plain data" for a function, coroutine or userdata anywhere in `value`, or a
key that is a table; "json.encode: <where>: ..." for a number that is not finite (`math.huge`, `0/0`), a string that
is not UTF-8 text, or tables nested more than 64 deep (a table that contains itself).

**Example**

```lua
local fights = { { mob = "Goblin", gil = 12 }, { mob = "Orc", gil = 30 } }
fs.write("fights.json", json.encode(fights, { pretty = true }))
```

How tables come out:

- A table whose keys are exactly 1 to n (a list without holes) is written as an array.
- Any other table is written as an object, its number and boolean keys as text: `{[5] = "x"}` becomes `{"5":"x"}`.
  An empty table becomes `{}`.
- Object keys come out sorted.
- `nil` is `null`; integers stay integers and other numbers are written with a decimal point (`1.0`).

## `json.decode`

```lua
json.decode(text) -> any
```

Reads JSON text into Lua values: objects become tables with string keys, arrays become lists (from index 1), `null`
becomes `nil`.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `text` | `string` | yes | The JSON text. |

**Returns** `any`: the value. `null` (at the top or inside) is `nil`, so a `null` in an array leaves a hole.

**Errors** "json.decode: ..." with the reason when `text` is not valid JSON.

**Example**

```lua
local raw = fs.read("fights.json")
if raw then
  local ok, fights = pcall(json.decode, raw)
  if ok then print(#fights .. " fights, first: " .. fights[1].mob) end
end
```

Object keys stay strings: `{"5": "x"}` gives a table read with `t["5"]`, not `t[5]`. Whole numbers come back as Lua
integers, numbers with a decimal point as floats.
