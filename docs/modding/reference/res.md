# `res`

`res` looks up the game's fixed data, read from the game's files: item, spell and ability records; the names of
status effects, zones, titles, key items, mounts, monster abilities, skills, jobs, elements, weather, weekdays and moon
phases; battle and system messages; the auto-translate dictionary; and raw game files. Most calls need the game's
loaded data, so they work in the mod phase (event handlers, commands, timers, `frame`) and raise "game state is not
available here" in the lobby and inside chat filters; `res.weather`, `res.weather_element`, `res.element`, `res.job`
and `res.model_files` work anywhere. An id the game has no entry for gives `nil`, and so does a lookup made while that
part of the game data is still loading (just after login). No permission is needed. Names come in the client's
language, except for `res.name`, which can ask for any of the four.

```lua
events.on("buff_gain", function(id)
  print("You gain " .. (res.status(id) or ("status " .. id)))
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`res.item(id)`](#resitem) | An item's record: name, description, stack size, equipment data. |
| [`res.spell(id)`](#resspell) | A spell's record: name, MP, cast time, recast, levels by job. |
| [`res.ability(id)`](#resability) | A job ability, weapon skill or pet command's record. |
| [`res.status(id)`](#resstatus) | A status effect's name. |
| [`res.status_info(id)`](#resstatus_info) | Whether a status effect can be cancelled or ever shows a time, and its description. |
| [`res.zone(id)`](#reszone) | A zone's name. |
| [`res.zone_text(zone, id)`](#reszone_text) | A zone dialogue entry's bytes, as the game file holds them. |
| [`res.zone_text_find(zone, prefix)`](#reszone_text_find) | The first zone dialogue entry starting with some bytes. |
| [`res.title(id)`](#restitle) | A title's name. |
| [`res.key_item(id)`](#reskey_item) | A key item's name and description. |
| [`res.augments(item, extdata)`](#resaugments) | An item's augments, decoded from its extdata. |
| [`res.message(id, options?)`](#resmessage) | A battle or system message as the chat log would print it. |
| [`res.autotranslate(word?, group?)`](#resautotranslate) | Searches the auto-translate dictionary. |
| [`res.autotranslate_groups()`](#resautotranslate_groups) | The auto-translate dictionary's groups. |
| [`res.name(kind, id, lang?)`](#resname) | A name in a chosen language (Japanese, English, German, French). |
| [`res.dat(id)`](#resdat) | A game file's bytes, by file id. |
| [`res.dat_path(id)`](#resdat_path) | Where on disk a game file is read from. |
| [`res.model_files(model)`](#resmodel_files) | The game file ids a model or a character look loads. |
| [`res.weather(id)`](#resweather) | A weather's name. |
| [`res.weather_element(id)`](#resweather_element) | The element a weather belongs to. |
| [`res.element(id)`](#reselement) | An element's name. |
| [`res.mount(id)`](#resmount) | A mount's name. |
| [`res.mob_skill(id)`](#resmob_skill) | A monster ability's name. |
| [`res.skill(id)`](#resskill) | A combat or magic skill's name. |
| [`res.day(weekday)`](#resday) | A Vana'diel weekday's name. |
| [`res.moon(phase)`](#resmoon) | A moon phase's name. |
| [`res.job(id)`](#resjob) | A job's three-letter abbreviation. |

## `res.item`

```lua
res.item(id) -> table?
```

Reads an item's record from the game's item tables. Use it to show an item's name, check its stack size, or read the
level, jobs and slots of a piece of equipment. The ids are the ones `game.items` gives in `Item.id`.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The item id, 0..65535 (65535 is gil). |

**Returns** `table?`: the item's record, or `nil` when there is no item with this id or the item tables have not
loaded yet. A field the record's kind does not have is `0`.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `integer` | The item id. |
| `name` | `string` | The item's name. |
| `log_name` | `string` | The long name the chat log uses ("Hachiman Domaru +1" for "Hachi. Domaru +1"). |
| `description` | `string` | Its description text (may be empty). |
| `kind` | `string` | Which layout the record has, set by the id range: `"general"` (0x0000-0x0FFF, 0x2200-0x27FF, 0x7800-0x7BFF), `"usable"` (0x1000-0x1FFF), `"automaton"` (0x2000-0x21FF, automaton heads, frames and attachments), `"armor"` (0x2800-0x3FFF, 0x5A00-0x6FFF), `"weapon"` (0x4000-0x59FF), `"maze"` (0x7000-0x73FF, Maze Monger tabulae, vouchers and runes), `"instinct"` (0x7400-0x77FF, Monstrosity instincts), `"objective"` (0xE000-0xEFF7, Records of Eminence objectives), `"instinct_catalogue"` (0xEFF8-0xEFFF), `"monster"` (0xF000-0xF1FF, Monstrosity species), `"roe_category"` (0xF200-0xF5FF, Records of Eminence categories), `"teleport_point"` (0xF600-0xF613), `"primer_category"` (0xF620-0xF62F) and `"primer_topic"` (0xF630-0xF71F) for the Adventuring Primer, `"currency"` (0xFFFF, gil). |
| `flags` | `integer` | Flag bits: 0x8000 Rare, 0x4000 Ex. |
| `stack` | `integer` | How many fit in one stack. |
| `type` | `integer` | The item type: 4 for weapons, 5 for armour, among others. |
| `targets` | `integer` | Bits for what the item can be used on. |
| `level` | `integer` | The level needed to equip it. |
| `slots` | `integer` | Equipment slot bits: bit n set means it fits slot n (0 main, 1 sub, 2 range, 3 ammo, 4 head, 5 body, 6 hands, 7 legs, 8 feet, 9 neck, 10 waist, 11 left ear, 12 right ear, 13 left ring, 14 right ring, 15 back). |
| `races` | `integer` | Race bits: bit n set means race n can wear it (races 1..8); 0x1FE is every race. |
| `jobs` | `integer` | Job bits: bit n set means job n can use it (1 WAR .. 22 RUN, see [`res.job`](#resjob)). |
| `superior_level` | `integer` | The superior level of the equipment. |
| `damage` | `integer` | A weapon's damage. |
| `delay` | `integer` | A weapon's delay. |
| `skill` | `integer` | A weapon's combat skill id (see [`res.skill`](#resskill)). |
| `max_charges` | `integer` | Charges of a usable piece of equipment; 255 means unlimited. |
| `cast_time` | `integer` | The use time of a usable piece of equipment, as the game file stores it. |
| `use_delay` | `integer` | Seconds after equipping before it can be used. |
| `reuse_delay` | `integer` | Seconds between uses. |

**Errors** "game state is not available here" in the lobby or inside a filter. An `id` that is not a whole number from
0 up raises an error.

**Example**

```lua
commands.register("iteminfo", function(arg)
  local it = res.item(tonumber(arg) or 0)
  if not it then
    print("No such item.")
    return
  end
  print(it.name .. ", stacks to " .. it.stack)
  if it.flags & 0x8000 ~= 0 then print("Rare") end
  if it.jobs & (1 << 3) ~= 0 then print("White Mage can use it") end
end)
```

Bit tests use Lua 5.4's `&` and `<<` operators.

## `res.spell`

```lua
res.spell(id) -> table?
```

Reads a spell's record. Use it for a spell's name, MP cost, cast time and recast, or the level at which each job learns
it. The ids are the ones `game.spells` returns and the keys of `game.recasts().spells`.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The spell id, 0..65535. |

**Returns** `table?`: the spell's record, or `nil` when there is no spell with this id or the spell tables have not
loaded yet.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `integer` | The spell id. |
| `name` | `string?` | Its name; `nil` while the name tables are loading. |
| `description` | `string?` | Its description; `nil` while the name tables are loading or when it has none. |
| `kind` | `integer` | 1 white magic, 2 black magic, 3 summoning, 4 ninjutsu, 5 song, 6 blue magic, 7 geomancy, 8 trust. |
| `element` | `integer` | Its element id (see [`res.element`](#reselement)). |
| `targets` | `integer` | Valid-target bits: 1 self, 2 player, 4 party, 8 alliance, 0x10 PC, 0x20 enemy (other bits exist). |
| `skill` | `integer` | Its magic skill id (see [`res.skill`](#resskill)). |
| `mp` | `integer` | The MP cost. |
| `cast_time` | `number` | The cast time in seconds. |
| `recast` | `number` | The recast time in seconds. |
| `range` | `integer` | The spell's range class, 0..15, as the game file stores it (an index into the game's range table, not a distance in yalms). |
| `area` | `boolean` | It always hits an area (the -ga spells, Protectra and the like). |
| `levels` | `table<integer, integer>` | The level each job learns it at, keyed by job id (1 WAR .. 22 RUN). Jobs that cannot learn the spell have no key. |
| `main_only` | `integer?` | The job that casts it only as its main job (20 SCH, 21 GEO), else `nil`. |
| `job_points` | `table<integer, boolean>` | Jobs for which `levels` holds the job points the spell takes at level 99 (a gift), keyed by job id. |

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
local s = res.spell(1)
if s then
  print(s.name, s.mp .. " MP", s.cast_time .. " s to cast")
  for job, level in pairs(s.levels) do
    print(res.job(job), level)
  end
end
```

## `res.ability`

```lua
res.ability(id) -> table?
```

Reads the record of a job ability, weapon skill or pet command. The ids are the ones `game.abilities` returns: weapon
skills 0..511 (0x000-0x1FF), job abilities from 512 (0x200), pet commands from 1024 (0x400).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The ability id, 0..65535. |

**Returns** `table?`: the ability's record, or `nil` when there is none with this id or the tables have not loaded yet.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `integer` | The ability id. |
| `name` | `string?` | Its name; `nil` while the name tables are loading. |
| `description` | `string?` | Its description; `nil` while the name tables are loading or when it has none. |
| `kind` | `integer` | 1 job ability, 2 pet command, 3 weapon skill, 4 trait, 20 (0x14) Monstrosity; 6..23 are rows that open a sub-list. |
| `cost` | `integer` | The MP cost, or the charges a Ready move uses. |
| `recast_id` | `integer` | The recast timer it uses: the key into `game.recasts().abilities`. 900 means none. |
| `recast` | `integer?` | Its base recast in seconds, before gear, merits and job points, as LandSandBoat has it (the game's files hold no recast lengths for abilities). `nil` for one without a recast (weapon skills, traits, Ready's charges). The length running now is in `game.recasts().totals`. |
| `targets` | `integer` | Valid-target bits, the same bits as [`res.spell`](#resspell)'s `targets`. |
| `tp` | `integer` | The TP cost; 255 means none. |
| `sub` | `integer` | Not 0 on a row that opens a sub-list (Phantom Roll, Blood Pact: Rage, Ready...): the type of the list it opens, whose rows have that `kind`. Such a row is not an ability itself; `/ja` skips it. |
| `level` | `integer` | The level the record gives (used by Monstrosity moves). |

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
commands.register("ready", function()
  local left = game.recasts().abilities
  for _, id in ipairs(game.abilities()) do
    local a = res.ability(id)
    if a and a.kind == 1 and not left[a.recast_id] then
      print(a.name .. " is ready")
    end
  end
end)
```

## `res.status`

```lua
res.status(id) -> string?
```

The name of a status effect, with each word capitalised. The ids are the ones `game.buffs` returns and
the `buff_gain` / `buff_lose` events pass.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The status effect id, 0..65535. |

**Returns** `string?`: the name, or `nil` when the id has no name or the battle text has not loaded yet.

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
for _, b in ipairs(game.buffs()) do
  local left = b.remaining and string.format(" (%.0f s)", b.remaining) or ""
  print((res.status(b.id) or ("status " .. b.id)) .. left)
end
```

## `res.status_info`

```lua
res.status_info(id) -> table?
```

What the game's status info file says of a status effect: the facts its status icons go by.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The status effect id, 0..65535. |

**Returns** `table?`: `nil` when the game has no entry for it (or its files have not loaded yet); otherwise:

| Field | Type | Description |
| --- | --- | --- |
| `cancellable` | `boolean` | The player can cancel it (from its icon, or with `/cancel`). |
| `untimed` | `boolean` | Its icon never shows a time, even when the server sent an end time. |
| `description` | `string` | Its description, as the help line shows it for its icon. |

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
for _, b in ipairs(game.buffs()) do
  local info = res.status_info(b.id)
  if b.remaining and not (info and info.untimed) then
    print(res.status(b.id), math.floor(b.remaining) .. "s")
  end
end
```

## `res.zone`

```lua
res.zone(id) -> string?
```

A zone's name, by the zone id `game.zone` returns and the `zone_change` event passes.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The zone id. |

**Returns** `string?`: the zone's name, or `nil` when the id has none or the name tables have not loaded yet.

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
events.on("zone_change", function(zone, previous)
  print("Entered " .. (res.zone(zone) or "?") .. " from " .. (previous and res.zone(previous) or "?"))
end)
```

## `res.zone_text`

```lua
res.zone_text(zone, id) -> string?
```

A zone's dialogue entry (the lines NPCs and events print there), by its id in the installed game files, as the file
holds it: Shift-JIS bytes with the game's codes in them (`0x07` a new line, `0x0a n` a value, `0x7f ...` the other
codes). [`text.from_shift_jis`](text.md#textfrom_shift_jis) turns the plain parts into text.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `zone` | `integer` | yes | The zone id. |
| `id` | `integer` | yes | The entry's id, as the [`zone_text`](events.md#zone_text) event gives it. |

**Returns** `string`, or `nil` when the zone has no such entry.

**Example**

```lua
events.on("zone_text", function(id)
  local raw = res.zone_text(game.player().zone, id)
  if raw then log.info(text.from_shift_jis((raw:gsub("[\0-\31\127]", "")))) end
end)
```

## `res.zone_text_find`

```lua
res.zone_text_find(zone, prefix) -> integer?
```

The id of the first entry of a zone's dialogue that starts with `prefix` (compared as bytes).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `zone` | `integer` | yes | The zone id. |
| `prefix` | `string` | yes | The bytes the entry starts with (Shift-JIS; plain English text is the same bytes). |

**Returns** `integer`, or `nil` when no entry starts with it.

**Example**

```lua
-- The training regime block of this zone's dialogue.
local r = res.zone_text_find(game.player().zone, "New training regime registered!")
```

## `res.title`

```lua
res.title(id) -> string?
```

A title's name, by title id.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The title id. |

**Returns** `string?`: the title, or `nil` when the id has none or the name tables have not loaded yet.

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
for id = 1, 5 do
  print(id, res.title(id))
end
```

## `res.key_item`

```lua
res.key_item(id) -> table?
```

A key item's name and description. The ids are the ones `game.key_items` returns.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The key item id. |

**Returns** `table?`: `nil` when the id has no key item or the name tables have not loaded yet.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `integer` | The key item id. |
| `name` | `string` | Its name. |
| `description` | `string?` | Its description, `nil` when it has none. |

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
commands.register("keyitems", function()
  for _, id in ipairs(game.key_items()) do
    local k = res.key_item(id)
    print(id, k and k.name or "?")
  end
end)
```

## `res.augments`

```lua
res.augments(item, extdata) -> table
```

Decodes the augments of an item from its extdata (the 24 raw bytes `game.items` gives in `Item.extdata`): the lines
the item window shows, and each augment's id and value.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `item` | `integer` | yes | The item id. |
| `extdata` | `string` | yes | The item's extdata: at least 24 bytes (only the first 24 are read). |

**Returns** `table`: always a table. It is empty when the item carries no augments (only weapons and armour with
augment data, and the item type 26, have them).

| Field | Type | Description |
| --- | --- | --- |
| `lines` | `string[]` | The augment lines as the item window shows them. |
| `short` | `string?` | The short line the item window shows in angle brackets, when the item has one (an evolith, a serial number, a rank). |
| `pairs` | `{id: integer, value: integer}[]` | Each augment: `id`, its augment message id, and `value`. |

**Errors** "game state is not available here" in the lobby or inside a filter; "the game's UI data is not available
here (only in the mod phase)"; "augment texts not loaded yet" while the augment texts are still loading; "extdata is
24 bytes" when `extdata` is shorter than 24 bytes.

**Example**

```lua
commands.register("augs", function()
  for _, it in ipairs(game.items(0)) do
    local info = it.extdata and res.item(it.id)
    if info then
      for _, line in ipairs(res.augments(it.id, it.extdata).lines or {}) do
        print(info.name .. ": " .. line)
      end
    end
  end
end)
```

## `res.message`

```lua
res.message(id, options?) -> table?
```

Expands a battle or system message (the ids in actions, `battle_message` and `damage` events) into the text the chat
log would print, with the names and numbers you pass filled in.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The message id, 0..65535. |
| `options` | `table` | no | Who and what to fill in. |

**`options` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `actor` | `integer` or `string` | none | Who acts: a server id (our id gives our name; another id must be an entity the client knows, or no name is filled in), or a name, used as a player's name. |
| `target` | `integer` or `string` | none | Who it is done to, as `actor`. |
| `values` | `integer[]` | none | The numbers the message fills in, in order (at most 5 are used). For a `battle_message`, its `param1`, `param2`. For an action's result, as the game fills it: the action's `param`, the result's `value`, its `proc.value`, its `react.value`, its `bit` (a move's or spell's name comes from these). |

**Returns** `table?`: `nil` when the id has no text or the battle text has not loaded yet.

| Field | Type | Description |
| --- | --- | --- |
| `text` | `string` | The expanded text; a message of several lines has them joined with `"\n"`. |
| `mode` | `integer?` | The chat mode the message text sets for itself, `nil` when it sets none. |

**Errors** "game state is not available here" in the lobby or inside a filter. A `values` entry that is not an
integer raises an error.

**Example**

```lua
events.on("battle_message", function(actor, target, message, p1, p2)
  local m = res.message(message, { actor = actor, target = target, values = { p1, p2 } })
  if m then
    log.info(m.text)
  end
end)
```

Pass server ids rather than names for monsters and NPCs: with an id the text gets the entity's proper wording (such as
"the" before a monster's name); a string is always used as a player's name.

## `res.autotranslate`

```lua
res.autotranslate(word?, group?) -> table[]?
```

Searches the auto-translate dictionary, as the chat line's completion list does. With a `group`, it lists that group's
entries. Otherwise it lists the group titles and phrases whose completion key starts with `word` (ignoring case),
sorted, each text once. With no `word` it lists every group's title; a `word` that matches nothing gives an empty list.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `word` | `string` | no | The start of the words to look for. Ignored when `group` is given. |
| `group` | `integer` | no | A group id (from [`res.autotranslate_groups`](#resautotranslate_groups)): list that group's entries. |

**Returns** `table[]?`: a list of entries, or `nil` while the dictionary has not loaded. An unknown `group` gives an
empty list.

| Field | Type | Description |
| --- | --- | --- |
| `text` | `string` | The entry's text. |
| `group` | `integer` | Its group id. |
| `id` | `integer` | Its phrase id within the group; `0` for a group's title. |
| `phrase` | `string` | The phrase as a chat line holds it: put it in chat text (for example with `chat.add`). |

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
local hits = res.autotranslate("thank")
for _, e in ipairs(hits or {}) do
  if e.id ~= 0 then
    chat.add("Found: " .. e.phrase)
  end
end
```

Since a search with no match falls back to the group titles, check `id ~= 0` to keep only phrases. Only the English
dictionary is loaded.

## `res.autotranslate_groups`

```lua
res.autotranslate_groups() -> table[]?
```

The groups (categories) of the auto-translate dictionary.

**Returns** `table[]?`: one entry per group, in the dictionary's order, or `nil` while the dictionary has not loaded.

| Field | Type | Description |
| --- | --- | --- |
| `group` | `integer` | The group id (for `res.autotranslate` and `text.phrase`). |
| `title` | `string` | The group's title. |
| `count` | `integer` | How many phrases it has. |

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
for _, g in ipairs(res.autotranslate_groups() or {}) do
  print(g.group, g.title, g.count .. " phrases")
end
```

## `res.name`

```lua
res.name(kind, id, lang?) -> string?
```

A name from the name tables of a chosen language. The first call for a language other than the client's starts
loading its tables in the background and returns `nil`; ask again a moment later.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `kind` | `string` | yes | `"item"`, `"key_item"`, `"spell"`, `"ability"` (the ids of [`res.ability`](#resability)), `"zone"` or `"title"`. |
| `id` | `integer` | yes | The id of that kind. |
| `lang` | `string` | no, default `"en"` | `"ja"`, `"en"`, `"de"` or `"fr"`. |

**Returns** `string?`: the name, or `nil` when the id has none or the language's tables are not loaded yet.

**Errors** "game state is not available here" in the lobby or inside a filter; "lang: ja, en, de or fr" for another
language; "kind: item, key_item, spell, ability, zone or title" for another kind.

**Example**

```lua
commands.register("jp", function(arg)
  local id = tonumber(arg) or 0
  local name = res.name("item", id, "ja")
  if not name then
    sleep(1)
    name = res.name("item", id, "ja")
  end
  print(name or "no name")
end)
```

## `res.dat`

```lua
res.dat(id) -> string?
```

Reads a whole game file by its file id, as the client reads it: when an installed game-file mod replaces the file, you
get the replacement. Use it to parse game data the API does not cover.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The game file id. |

**Returns** `string?`: the file's bytes (a binary Lua string), or `nil` when the install has no such file or it cannot
be read.

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
local bytes = res.dat(73)
if bytes then
  print("file 73: " .. #bytes .. " bytes, first byte " .. string.byte(bytes, 1))
end
```

## `res.dat_path`

```lua
res.dat_path(id) -> string?
```

Where on disk a game file id is read from: the game's file, or the file the client reads in its place when an
installed game-file mod changes it.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The game file id. |

**Returns** `string?`: the path, or `nil` when the id is not in the game's file table.

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
print(res.dat_path(73) or "no such file")
```

## `res.model_files`

```lua
res.model_files(model) -> table
```

The game file ids a model loads. Give an NPC or monster model id (`Entity.model`) for its one file, or a character look
(`Entity.look` with `race` added from `Entity.race`) for its skeleton and each part's file.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `model` | `integer` or `table` | yes | An NPC model id, or a look table. |

**`model` fields** (when it is a table)

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `race` | `integer` | `0` | The race (1..8 for the player races). |
| `face` | `integer` | `0` | The face model id. |
| `head` | `integer` | `0` | The head model id. |
| `body` | `integer` | `0` | The body model id. |
| `hands` | `integer` | `0` | The hands model id. |
| `legs` | `integer` | `0` | The legs model id. |
| `feet` | `integer` | `0` | The feet model id. |
| `main` | `integer` | `0` | The main weapon model id. |
| `sub` | `integer` | `0` | The sub weapon model id. |
| `range` | `integer` | `0` | The ranged weapon model id. |

**Returns** `table`. For a model id: `{file = integer}`. For a look:

| Field | Type | Description |
| --- | --- | --- |
| `skeleton` | `integer` | The skeleton's file id. |
| `parts` | `{slot: string, model: integer, file: integer}[]` | One entry per part loaded: `slot` is `"face"`, `"head"`, `"body"`, `"hands"`, `"legs"`, `"feet"`, `"main"`, `"sub"` or `"range"`; `model` the model id; `file` its file id. |

A `main` or `sub` of `0` is left out of `parts`. For races that are not player races the weapon slots are left out.

**Errors** "res.model_files takes an NPC model id or a look {race, face, head, ...}" when `model` is neither an
integer nor a table (a float such as `100.0` counts as neither); "race N has no model tables (0..8, 29..36)" for a
race without models; "<slot> model id N is outside the race R table" for a part id the race does not have.

**Example**

```lua
commands.register("files", function(arg)
  local e = game.entity(tonumber(arg) or 0)
  if not e then return end
  if e.look then
    local look = e.look
    look.race = e.race
    for _, p in ipairs(res.model_files(look).parts) do print(p.slot, p.model, p.file) end
  else
    print("model file " .. res.model_files(e.model).file)
  end
end)
```

This function needs no game state: it works anywhere.

## `res.weather`

```lua
res.weather(id) -> string?
```

A weather's English name, by the weather id `game.weather` returns and the `weather_change` event passes.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | 0 Clear, 1 Sunshine, 2 Clouds, 3 Fog, 4 Hot Spell, 5 Heat Wave, 6 Rain, 7 Squall, 8 Dust Storm, 9 Sand Storm, 10 Wind, 11 Gales, 12 Snow, 13 Blizzards, 14 Thunder, 15 Thunderstorms, 16 Auroras, 17 Stellar Glare, 18 Gloom, 19 Darkness. |

**Returns** `string?`: the name, or `nil` for an id above 19.

**Example**

```lua
events.on("weather_change", function(weather)
  print("Weather: " .. (res.weather(weather) or "?"))
end)
```

Works anywhere, also in the lobby.

## `res.weather_element`

```lua
res.weather_element(id) -> integer?
```

The element a weather belongs to, as an element id (see [`res.element`](#reselement)).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The weather id (see [`res.weather`](#resweather)). |

**Returns** `integer?`: 0 Fire (4, 5), 1 Ice (12, 13), 2 Wind (10, 11), 3 Earth (8, 9), 4 Lightning (14, 15), 5 Water
(6, 7), 6 Light (16, 17), 7 Dark (18, 19); `nil` for weather 0..3 (no element) and for an id above 19.

**Example**

```lua
local e = res.weather_element(game.weather())
print(e and (res.element(e) .. " weather") or "no elemental weather")
```

Works anywhere, also in the lobby.

## `res.element`

```lua
res.element(id) -> string?
```

An element's English name.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | 0 Fire, 1 Ice, 2 Wind, 3 Earth, 4 Lightning, 5 Water, 6 Light, 7 Dark. |

**Returns** `string?`: the name, or `nil` for an id above 7.

**Example**

```lua
for id = 0, 7 do
  print(id, res.element(id))
end
```

Works anywhere, also in the lobby.

## `res.mount`

```lua
res.mount(id) -> string?
```

A mount's name, by its position in the game's mount list (counting from 0).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The mount's index in the mount list, from 0. |

**Returns** `string?`: the name, or `nil` when the list has no such entry or has not loaded yet.

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
for i = 0, 4 do
  print(i, res.mount(i))
end
```

## `res.mob_skill`

```lua
res.mob_skill(id) -> string?
```

The name of a monster ability. Monster ability ids start at 256.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The monster ability id, 256 and up. |

**Returns** `string?`: the name, or `nil` for an id below 256, an unused entry, or while the battle text has not loaded.

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
for id = 256, 260 do
  print(id, res.mob_skill(id))
end
```

## `res.skill`

```lua
res.skill(id) -> string?
```

The name of a combat or magic skill, by skill id (the `skill` field of [`res.item`](#resitem) and
[`res.spell`](#resspell)).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The skill id. |

**Returns** `string?`: the name, or `nil` when the id has none or the battle text has not loaded.

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
local s = res.spell(1)
if s then print(s.name .. " uses " .. (res.skill(s.skill) or "?")) end
```

## `res.day`

```lua
res.day(weekday) -> string?
```

The name of a Vana'diel weekday, by the number `game.time().weekday` gives and the `day_change` event passes.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `weekday` | `integer` | yes | 0 Firesday .. 7 Darksday. |

**Returns** `string?`: the name, or `nil` for another number or while the text has not loaded.

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
events.on("day_change", function(weekday)
  print("Today is " .. (res.day(weekday) or "?"))
end)
```

## `res.moon`

```lua
res.moon(phase) -> string?
```

The name of a moon phase, by the number `game.time().moon_phase` gives and the `moon_change` event passes.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `phase` | `integer` | yes | The moon phase, 0..11. |

**Returns** `string?`: the name, or `nil` for another number or while the text has not loaded.

**Errors** "game state is not available here" in the lobby or inside a filter.

**Example**

```lua
local t = game.time()
print((res.moon(t.moon_phase) or "?") .. ", " .. t.moon .. "%")
```

## `res.job`

```lua
res.job(id) -> string?
```

A job's three-letter abbreviation.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | 1 WAR, 2 MNK, 3 WHM, 4 BLM, 5 RDM, 6 THF, 7 PLD, 8 DRK, 9 BST, 10 BRD, 11 RNG, 12 SAM, 13 NIN, 14 DRG, 15 SMN, 16 BLU, 17 COR, 18 PUP, 19 DNC, 20 SCH, 21 GEO, 22 RUN. |

**Returns** `string?`: the abbreviation, or `nil` for 0 and for an id above 22.

**Example**

```lua
local p = game.player()
if p.main_job then
  local sub = p.sub_job and res.job(p.sub_job) or "-"
  print(res.job(p.main_job) .. tostring(p.main_level) .. "/" .. sub)
end
```

Works anywhere, also in the lobby.
