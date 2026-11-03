# `game`

The `game` namespace reads the game's state: your character, the zone, the entities around you, your party, bags and equipment, status effects, recasts, jobs and skills, the clock, maps and the zone's collision. Every function here only reads: none of them changes the game, needs a permission or waits for the next frame, and each call builds a fresh table with this frame's values. The calls work in event handlers, commands, timers, window and HUD handlers, key and mouse filters and packet hooks; in the lobby (before login) and inside chat filters they raise "game state is not available here (in the lobby, or outside the mod phase)". `game.map` and `game.map_pos` need a little more: they also raise an error in key and mouse filters and in packet hooks.

```lua
commands.register("whereami", function()
  local p = game.player()
  local zone = p.zone and res.zone(p.zone) or "?"
  local grid = game.info().map_grid or ""
  print(("%s is in %s %s at x %.1f, z %.1f"):format(p.name, zone, grid, p.pos.x, p.pos.z))
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`game.player()`](#gameplayer) | Your character: ids, zone, position, vitals, jobs, target. |
| [`game.zone()`](#gamezone) | The current zone's id. |
| [`game.entity(key)`](#gameentity) | One entity by server id or index. |
| [`game.entities(filter?)`](#gameentities) | Every entity in the zone, optionally filtered by distance and kind. |
| [`game.target()`](#gametarget) | Your current target. |
| [`game.party()`](#gameparty) | Your party and alliance members. |
| [`game.items(bag)`](#gameitems) | The items in one bag. |
| [`game.bag_size(bag)`](#gamebag_size) | How many slots a bag has. |
| [`game.equipment()`](#gameequipment) | What you have equipped, by equipment slot. |
| [`game.buffs()`](#gamebuffs) | Your status effects and their time left. |
| [`game.recasts()`](#gamerecasts) | Seconds left on your spell and ability recasts. |
| [`game.time()`](#gametime) | The Vana'diel date, clock and moon. |
| [`game.weather()`](#gameweather) | The current weather id. |
| [`game.key_items()`](#gamekey_items) | The key items you hold. |
| [`game.treasure()`](#gametreasure) | The items in the treasure pool. |
| [`game.event()`](#gameevent) | The event (cutscene, NPC talk) under way, and its menu. |
| [`game.entity_by_tag(tag)`](#gameentity_by_tag) | The entity a target tag such as `<t>` or `<bt>`, a player's name or an id names. |
| [`game.skills()`](#gameskills) | Your combat, magic and crafting skills. |
| [`game.jobs()`](#gamejobs) | The level of every unlocked job, and master levels. |
| [`game.stats()`](#gamestats) | Your attributes, attack, defense, resistances, experience and more. |
| [`game.merits()`](#gamemerits) | Your merits' levels and next costs. |
| [`game.job_points()`](#gamejob_points) | Capacity points and job points per job. |
| [`game.gil()`](#gamegil) | Your gil. |
| [`game.spells()`](#gamespells) | The spells you have learned. |
| [`game.abilities()`](#gameabilities) | The abilities in your ability lists. |
| [`game.job_data()`](#gamejob_data) | Your main and sub job's setup (blue magic, automaton parts). |
| [`game.macros()`](#gamemacros) | The macro palette: book, set and every macro of the set. |
| [`game.map()`](#gamemap) | The map shown for where you stand. |
| [`game.map_pos(x, z)`](#gamemap_pos) | A world position as a pixel on that map. |
| [`game.collision(x, z, radius?)`](#gamecollision) | The zone's collision triangles near a point. |
| [`game.collision_mesh()`](#gamecollision_mesh) | Every collision triangle of the zone, packed into a string. |
| [`game.raycast(from, to)`](#gameraycast) | Where a line segment first hits the zone's collision. |
| [`game.floor(x, z, y?)`](#gamefloor) | The height a character standing at a point would stand on. |
| [`game.cast()`](#gamecast) | Your cast bar while it shows. |
| [`game.info()`](#gameinfo) | Session flags: server, Mog House, chat, menus, combat, lock-on and more. |
| [`game.camera()`](#gamecamera) | The camera's eye and look points. |
| [`game.shop()`](#gameshop) | The rows of the open NPC shop. |
| [`game.npc_names()`](#gamenpc_names) | The zone's NPC names by server id. |

## `game.player`

```lua
game.player() -> Player
```

Your own character: name, ids, zone, position, heading, status, HP, MP and TP, their maxima, your jobs and your target. Call it whenever you need "me".

**Returns** `Player`: see [Player](types.md#player). Never nil. `hp`, `mp`, `tp`, `hpp` and `mpp` are nil until the server has sent your vitals; `hp_max`, `mp_max` and the job fields are nil until it has sent your stats; `target` is nil without a target.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local p = game.player()
print(p.name, "HP", p.hp, "/", p.hp_max, "TP", p.tp)
if p.main_job then
  print(("%s%d/%s%d"):format(res.job(p.main_job), p.main_level, res.job(p.sub_job) or "-", p.sub_level))
end
```

## `game.zone`

```lua
game.zone() -> integer?
```

The id of the zone you are in. Use `res.zone(id)` for its name.

**Returns** `integer?`: the zone id. During a zone change it still gives the old zone until the new zone has loaded; the `zone_change` event fires at that moment. nil only before the first zone has loaded.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local z = game.zone()
if z then
  print("Zone", z, res.zone(z))
end
```

## `game.entity`

```lua
game.entity(key) -> Entity?
```

Looks up one entity (a player, NPC or monster the client knows) by its server id or by its index in the zone.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `key` | `integer` | yes | A server id or an index. Your own id and index are checked first, then server ids, then indexes. |

**Returns** `Entity?`: see [Entity](types.md#entity). Your own id or index gives your own entity (with `me = true`). nil when nothing with that id or index is in range.

**Errors** "game state is not available here" in the lobby or a chat filter; an error when `key` is not a whole number of 0 or more.

**Example**

```lua
local t = game.player().target
local e = t and game.entity(t)
if e then
  print(e.name, e.hpp .. "%", ("%.1f yalms"):format(e.distance))
end
```

## `game.entities`

```lua
game.entities(filter?) -> Entity[]
```

Every entity the client knows in the zone, as a list: yourself first, then everyone else in index order. Use the filter to keep only nearby entities or only some kinds.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `filter` | `table` | no | What to keep. Without it you get everything. |

**`filter` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `distance` | `number` | none | Keep only entities at most this many yalms from you (straight-line distance, height included). |
| `kinds` | `string[]` | all kinds | Keep only these kinds: any of `"player"`, `"npc"`, `"mob"`. You are included when `kinds` is absent or contains `"player"`. |

A field of the wrong type is ignored, as if it were absent.

**Returns** `Entity[]`: a list (index 1, 2, ...) of [Entity](types.md#entity) tables. Never nil; it is empty when nothing matches.

**Errors** "game state is not available here" in the lobby or a chat filter; an error when `filter` is given but is not a table.

**Example**

```lua
for _, e in ipairs(game.entities({ distance = 20, kinds = { "mob" } })) do
  if e.claim == 0 and e.hpp > 0 then
    print(e.name, e.index, ("%.1f yalms"):format(e.distance))
  end
end
```

## `game.target`

```lua
game.target() -> Entity?
```

The entity you have targeted.

**Returns** `Entity?`: see [Entity](types.md#entity). Your own entity when you target yourself. nil when you have no target, or when the target is no longer in range.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
events.on("target_change", function()
  local t = game.target()
  if t then
    print("Target:", t.name, t.kind, t.hpp .. "%")
  end
end)
```

## `game.party`

```lua
game.party() -> Member[]
```

The members of your party and alliance, in slot order. Slots keep the game's numbers: 0 to 5 are your own party (slot 0 is you), 6 to 11 and 12 to 17 the alliance's other two parties.

**Returns** `Member[]`: a list (index 1, 2, ...) of [Member](types.md#member) tables, only the slots in use. Never nil.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
for _, m in ipairs(game.party()) do
  if m.party == 0 then
    print(m.slot, m.name, m.hpp .. "%", m.in_zone and "" or "(elsewhere)")
  end
end
```

Members in another zone are listed too (`in_zone` is false). The members' own entities, when they are near, come from `game.entity(m.id)`.

## `game.items`

```lua
game.items(bag) -> Item[]
```

The items in one bag, in the order the game's item list shows them.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `bag` | `integer` | yes | The bag's number: 0 inventory, 1 Mog Safe, 2 Storage, 3 temporary items, 4 Mog Locker, 5 Mog Satchel, 6 Mog Sack, 7 Mog Case, 8 Mog Wardrobe, 9 Mog Safe 2, 10 to 16 Mog Wardrobes 2 to 8. |

**Returns** `Item[]`: a list (index 1, 2, ...) of [Item](types.md#item) tables, only slots that hold something. Never nil; empty for an empty bag or one the server has not sent.

**Errors** "game state is not available here" in the lobby or a chat filter; an error when `bag` is not a whole number from 0 to 255.

**Example**

```lua
for _, it in ipairs(game.items(0)) do
  local r = res.item(it.id)
  print(it.slot, r and r.name or it.id, it.count)
end
```

Slots count from 1. Slot 0, where the server keeps your gil, is never in the list: use `game.gil()` for the gil.

## `game.bag_size`

```lua
game.bag_size(bag) -> integer?
```

How many item slots a bag has.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `bag` | `integer` | yes | The bag's number, as for `game.items`. |

**Returns** `integer?`: the number of slots. 0 before the server has sent the bag sizes, and for a bag you do not have. nil for a bag number above 17.

**Errors** "game state is not available here" in the lobby or a chat filter; an error when `bag` is not a whole number from 0 to 255.

**Example**

```lua
print(("Inventory: %d / %d"):format(#game.items(0), game.bag_size(0) or 0))
```

## `game.equipment`

```lua
game.equipment() -> table
```

What you have equipped, by equipment slot.

**Returns** `table`: keyed by the game's equipment slot number, 0 to 15: 0 main, 1 sub, 2 range, 3 ammo, 4 head, 5 body, 6 hands, 7 legs, 8 feet, 9 neck, 10 waist, 11 left ear, 12 right ear, 13 left ring, 14 right ring, 15 back. A slot with nothing equipped has no entry. Each entry:

| Field | Type | Description |
| --- | --- | --- |
| `bag` | `integer` | The bag the piece is in. |
| `slot` | `integer` | Its slot in that bag. |
| `id` | `integer?` | Its item id; nil when that bag slot's item is not known. |

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local eq = game.equipment()
for slot = 0, 15 do
  local e = eq[slot]
  local r = e and e.id and res.item(e.id)
  print(slot, r and r.name or "-")
end
```

The keys start at 0, so `ipairs` would skip the main hand: loop from 0 to 15 as above, or use `pairs`.

## `game.buffs`

```lua
game.buffs() -> table[]
```

Your status effects, in the order of your status icons.

**Returns** `table[]`: a list (index 1, 2, ...), never nil:

| Field | Type | Description |
| --- | --- | --- |
| `id` | `integer` | The status effect's id (`res.status(id)` gives its name). |
| `remaining` | `number?` | Seconds left, to about a second. nil when the effect has no known end. |

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
for _, b in ipairs(game.buffs()) do
  local name = res.status(b.id) or tostring(b.id)
  if b.remaining then
    print(name, math.floor(b.remaining) .. "s")
  else
    print(name)
  end
end
```

## `game.recasts`

```lua
game.recasts() -> table
```

Seconds left before you can use spells and abilities again.

**Returns** `table`, never nil:

| Field | Type | Description |
| --- | --- | --- |
| `spells` | `table<integer, number>` | Seconds left, by spell id. |
| `abilities` | `table<integer, number>` | Seconds left, by recast id (an ability's `recast_id` in `res.ability`). |

A spell or ability with no entry is ready. An entry can also be 0 (ready again): spells stay in the table after their recast ends, and abilities until the server next updates their recasts.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local left = game.recasts().spells[1] or 0 -- spell 1, Cure
if left > 0 then
  print(("Cure is ready in %.1f s"):format(left))
else
  print("Cure is ready")
end
```

## `game.time`

```lua
game.time() -> Time
```

The Vana'diel clock and calendar, and the moon.

**Returns** `Time`: see [Time](types.md#time). Before the server has sent the time, every field is worked out from 0.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local t = game.time()
print(("%d/%02d/%02d %02d:%02d, %s, %s (%d%%)"):format(
  t.year, t.month, t.day, t.hour, t.minute, res.day(t.weekday), res.moon(t.moon_phase), t.moon))
```

## `game.weather`

```lua
game.weather() -> integer
```

The current weather.

**Returns** `integer`: the weather id, 0 (clear) to 19 (darkness). `res.weather(id)` gives its name and `res.weather_element(id)` the element it brings.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local w = game.weather()
local el = res.weather_element(w)
print("Weather:", res.weather(w), el and res.element(el) or "no element")
```

## `game.key_items`

```lua
game.key_items() -> integer[]
```

The key items you hold.

**Returns** `integer[]`: a list of key item ids, never nil. `res.key_item(id)` gives each one's name and description.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
for _, id in ipairs(game.key_items()) do
  local ki = res.key_item(id)
  print(id, ki and ki.name or "?")
end
```

## `game.treasure`

```lua
game.treasure() -> table[]
```

The items in the treasure pool, with your lot and the highest lot so far.

**Returns** `table[]`: a list (index 1, 2, ...) with one entry per item in the pool, never nil:

| Field | Type | Description |
| --- | --- | --- |
| `slot` | `integer` | The pool slot, 0 to 9 (what `actions.lot` and `actions.pass` take). |
| `item` | `integer` | The item id. |
| `count` | `integer` | How many. |
| `dropper` | `integer` | The server id of what it came from (0 when none). |
| `lotted` | `boolean` | You have lotted on it. |
| `passed` | `boolean` | You have passed on it. |
| `lot` | `integer?` | Your lot; nil when you have none. |
| `high_lot` | `integer` | The highest lot so far; 0 when nobody has lotted. |
| `high_name` | `string` | Who has the highest lot; `""` when nobody. |

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
for _, t in ipairs(game.treasure()) do
  local r = res.item(t.item)
  local mine = t.lotted and ("lot " .. tostring(t.lot)) or (t.passed and "passed" or "undecided")
  print(t.slot, r and r.name or t.item, mine, "high:", t.high_name, t.high_lot)
end
```

## `game.event`

```lua
game.event() -> table?
```

The event (a cutscene or NPC conversation) under way, and the menu it shows, if any.

**Returns** `table?`: nil when no event runs. While one runs, a table that has a `menu` field only while the event shows a menu:

| Field | Type | Description |
| --- | --- | --- |
| `menu.question` | `string` | The menu's question. |
| `menu.options` | `string[]` | The options' texts, in order. |
| `menu.cursor` | `integer` | The option the cursor is on, from 1. |

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local ev = game.event()
if ev and ev.menu then
  print(ev.menu.question)
  for i, text in ipairs(ev.menu.options) do
    print(i == ev.menu.cursor and ">" or " ", i, text)
  end
end
```

Pick an option with `actions.choose(i)`, using the same numbers.

## `game.entity_by_tag`

```lua
game.entity_by_tag(tag) -> Entity?
```

The entity a target tag names, as text commands understand them, or a player found by name or by server id.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `tag` | `string` | yes | One of: a tag `t` (your target), `me`, `bt` (your party's battle target), `pet`, `ht` (the call-for-help target), `scan` (the Wide Scan tracked entity), with or without `<>`; a party slot `p0` to `p5` or alliance slot `a10` to `a15`, `a20` to `a25` (with or without `<>`); a player's name (any case); or a server id written as a number (decimal, or hex as `0x1A2B`). |

**Returns** `Entity?`: see [Entity](types.md#entity). nil when the tag names nobody, or the entity is not in range. A name finds players (and your own or your party's pets and trusts), not NPCs or monsters.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local bt = game.entity_by_tag("<bt>")
if bt then
  print("Battle target:", bt.name, bt.hpp .. "%")
end
local p1 = game.entity_by_tag("p1")
print("Party member 2 is", p1 and p1.name or "not here")
```

## `game.skills`

```lua
game.skills() -> table
```

Your skill levels: combat skills, magic skills and crafts.

**Returns** `table`, never nil, with three tables keyed by skill id (`res.skill(id)` gives a combat or magic skill's name):

| Field | Type | Description |
| --- | --- | --- |
| `combat` | `table<integer, table>` | Combat skills, ids 1 to 31: `{level, capped}`. |
| `magic` | `table<integer, table>` | Magic skills, ids 32 to 47: `{level, capped}`. |
| `crafts` | `table<integer, table>` | Crafts, ids 48 to 62: `{rank, level, capped}`. |

`level` is an integer, `capped` a boolean (the skill is at its cap for your level), `rank` the craft's rank number. Only skills the server lists are there; the tables are empty until it has sent your skills.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local s = game.skills()
for id, v in pairs(s.combat) do
  print(res.skill(id) or id, v.level, v.capped and "(capped)" or "")
end
```

## `game.jobs`

```lua
game.jobs() -> table
```

The level of every job you have unlocked, and your master levels.

**Returns** `table`, never nil:

| Field | Type | Description |
| --- | --- | --- |
| `levels` | `table<integer, integer>` | Level by job id (1 WAR to 22 RUN), for each unlocked job. |
| `master` | `table<integer, integer>` | Master level by job id, for each job with master levels unlocked. |

Both are empty until the server has sent your job data.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local j = game.jobs()
for job, level in pairs(j.levels) do
  local ml = j.master[job]
  print(res.job(job), level, ml and ("ML " .. ml) or "")
end
```

## `game.stats`

```lua
game.stats() -> table?
```

Your attributes and the other numbers of the Status window.

**Returns** `table?`: nil before the server has sent your stats. Otherwise:

| Field | Type | Description |
| --- | --- | --- |
| `base` | `integer[]` | Base attributes, 7 values: STR, DEX, VIT, AGI, INT, MND, CHR. |
| `bonus` | `integer[]` | The bonus to each, same order (can be negative). |
| `attack` | `integer` | Attack. |
| `defense` | `integer` | Defense. |
| `resist` | `integer[]` | Resistances, 8 values by element: index 1 is element 0 (Fire) ... index 8 is element 7 (Dark). |
| `item_level` | `integer` | Item level. |
| `superior_level` | `integer` | Superior level. |
| `title` | `integer` | Title id (`res.title(id)` gives its name). |
| `rank` | `integer` | Nation rank. |
| `rank_points` | `integer` | Rank points. |
| `nation` | `integer` | Nation number. |
| `home_point` | `integer` | The home point's zone id. |
| `exp` | `integer` | Experience points in this level. |
| `exp_next` | `integer` | Experience points needed for the next level. |
| `master_level` | `integer` | Master level. |
| `exemplar` | `integer` | Exemplar points. |
| `exemplar_next` | `integer` | Exemplar points needed for the next master level. |
| `limit_points` | `integer?` | Limit points. |
| `merit_points` | `integer?` | Merit points you have. |
| `merit_max` | `integer?` | The most merit points you can hold. |

`limit_points`, `merit_points` and `merit_max` are nil until the server has sent them.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local s = game.stats()
if s then
  local names = { "STR", "DEX", "VIT", "AGI", "INT", "MND", "CHR" }
  for i, n in ipairs(names) do
    print(n, s.base[i], ("%+d"):format(s.bonus[i]))
  end
  print("Attack", s.attack, "Defense", s.defense, "EXP", s.exp .. "/" .. s.exp_next)
end
```

## `game.merits`

```lua
game.merits() -> table
```

The merits the server lists for you, with their level and the cost of the next one.

**Returns** `table`, never nil: keyed by merit index (the server's merit id divided by 2), each `{level, next_cost}`:

| Field | Type | Description |
| --- | --- | --- |
| `level` | `integer` | How many times you have raised it. |
| `next_cost` | `integer` | Merit points the next raise costs. |

Empty until the server has sent the merit list.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local raised = 0
for _, m in pairs(game.merits()) do
  if m.level > 0 then raised = raised + 1 end
end
print(raised, "merits raised")
```

## `game.job_points`

```lua
game.job_points() -> table
```

Capacity points and job points for every job.

**Returns** `table`, never nil: keyed by job id (every id from 0 to 23 once the server has sent them; empty before), each:

| Field | Type | Description |
| --- | --- | --- |
| `capacity` | `integer` | Capacity points. |
| `points` | `integer` | Job points you have to spend. |
| `spent` | `integer` | Job points spent on this job. |

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local p = game.player()
local jp = p.main_job and game.job_points()[p.main_job]
if jp then
  print(("%s: %d JP, %d spent, %d CP"):format(res.job(p.main_job), jp.points, jp.spent, jp.capacity))
end
```

## `game.gil`

```lua
game.gil() -> integer
```

How much gil you have.

**Returns** `integer`: your gil; 0 before the server has sent it.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
print(("You have %d gil."):format(game.gil()))
```

## `game.spells`

```lua
game.spells() -> integer[]
```

The spells you have learned (whether or not your current jobs can cast them).

**Returns** `integer[]`: a list of spell ids, smallest first; never nil. `res.spell(id)` gives each spell's record.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local known = {}
for _, id in ipairs(game.spells()) do known[id] = true end
if not known[1] then
  print("You have not learned Cure.")
end
```

## `game.abilities`

```lua
game.abilities() -> integer[]
```

The abilities the game's ability lists offer you now: job abilities, weapon skills and pet commands, in the lists' order.

**Returns** `integer[]`: a list of ability ids (the ids `res.ability` and `actions.ability` take); never nil. Empty until the game data and the server's list of your abilities have arrived.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
for _, id in ipairs(game.abilities()) do
  local a = res.ability(id)
  if a then print(id, a.name) end
end
```

## `game.job_data`

```lua
game.job_data() -> table
```

The setup of your main and sub job as the server sent it: the blue magic set for Blue Mage, the automaton's parts for Puppetmaster, and the raw bytes for any job.

**Returns** `table`, never nil, with `main` and `sub` (each nil when the server has sent nothing for it):

| Field | Type | Description |
| --- | --- | --- |
| `job` | `integer` | The job id. |
| `raw` | `string` | The setup's 156 bytes as the server sent them. |
| `blue_spells` | `integer[]?` | Blue Mage (job 16) only: the spell ids set, in slot order, empty slots left out. |
| `head` | `integer?` | Puppetmaster (job 18) only: the automaton head's item id; nil when none. |
| `frame` | `integer?` | Puppetmaster only: the frame's item id; nil when none. |
| `attachments` | `integer[]?` | Puppetmaster only: the attachments' item ids, empty slots left out. |

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local jd = game.job_data()
if jd.main and jd.main.blue_spells then
  for _, id in ipairs(jd.main.blue_spells) do
    local s = res.spell(id)
    print(s and s.name or id)
  end
end
```

## `game.macros`

```lua
game.macros() -> table?
```

The macro palette: which book and set are current, the books' titles, and the 20 macros of the current set.

**Returns** `table?`: nil before the macros have loaded. Otherwise:

| Field | Type | Description |
| --- | --- | --- |
| `book` | `integer` | The current book, 1 to 40. |
| `set` | `integer` | The current set in it, 1 to 10. |
| `book_title` | `string` | The current book's title (`"Book01"` and so on when it has none). |
| `books` | `string[]` | All 40 book titles. |
| `ctrl` | `table[]` | The Ctrl row's macros, 1 to 10. |
| `alt` | `table[]` | The Alt row's macros, 1 to 10. |

Each macro is `{title = string, lines = string[]}`, `lines` its 6 lines (empty strings for empty lines).

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local m = game.macros()
if m then
  print(("Book %d (%s), set %d"):format(m.book, m.book_title, m.set))
  for i, mac in ipairs(m.ctrl) do
    print("Ctrl", i, mac.title, mac.lines[1])
  end
end
```

Run one with `actions.macro("ctrl", i)`; switch with `actions.macro_set(book, set)`.

## `game.map`

```lua
game.map() -> table?
```

The map the client shows for where you stand (a map the zone has and you own). Use it with `game.map_pos` and `hud.map` to draw on the map.

**Returns** `table?`: nil when there is no such map. Otherwise:

| Field | Type | Description |
| --- | --- | --- |
| `zone` | `integer` | The zone the map belongs to. |
| `number` | `integer` | The map's number within the zone. |
| `file` | `integer` | The map image's game file id (`res.dat(file)` reads it). |
| `scale` | `integer` | The map's scale value. |
| `origin` | `{x: integer, y: integer}` | The image's top left corner, in map pixels. |
| `size` | `integer` | The image's width and height in pixels: 512. |
| `px_per_unit` | `number` | Map pixels per yalm. |

**Errors** "game state is not available here" in the lobby or a chat filter; "the game's UI data is not available here (only in the mod phase)" in a key or mouse filter or a packet hook.

**Example**

```lua
local m = game.map()
if m then
  print(("Map %d of zone %d (file %d): %.2f pixels a yalm"):format(m.number, m.zone, m.file, m.px_per_unit))
end
```

## `game.map_pos`

```lua
game.map_pos(x, z) -> integer?, integer?
```

Where a world position falls on the current map's 512 x 512 image.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `x` | `number` | yes | World x. |
| `z` | `number` | yes | World z. |

**Returns** `integer?, integer?`: two values, the pixel's x and y on the image (0, 0 its top left). A point off the map gives values outside 0 to 511. Both nil when there is no map (see `game.map`).

**Errors** "game state is not available here" in the lobby or a chat filter; "the game's UI data is not available here (only in the mod phase)" in a key or mouse filter, a packet hook, the entry file or a `load` handler; an error when `x` or `z` is not a number.

**Example**

```lua
local p = game.player()
local px, py = game.map_pos(p.pos.x, p.pos.z)
if px then
  print("You are at pixel", px, py, "of the map")
end
```

## `game.collision`

```lua
game.collision(x, z, radius?) -> table[]?
```

The zone's collision triangles near a point (the surfaces characters walk on and bump into), for path finding, line-of-sight checks or drawing.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `x` | `number` | yes | World x of the centre. |
| `z` | `number` | yes | World z of the centre. |
| `radius` | `number` | no, default `8` | Keep triangles with a corner within this many yalms of (x, z), measured on the ground plane. Limited to 0 to 64. |

**Returns** `table[]?`: nil before the zone's collision has loaded. Otherwise a list of triangles (possibly empty), each:

| Field | Type | Description |
| --- | --- | --- |
| `[1]`, `[2]`, `[3]` | `{x, y, z}` | The three corners, in world coordinates (y down, as positions). |
| `normal` | `{x, y, z}` | The triangle's normal. |
| `kind` | `integer` | The surface type number. |

The surfaces of open doors are left out.

**Errors** "game state is not available here" in the lobby or a chat filter; an error when `x`, `z` or `radius` is not a number.

**Example**

```lua
local p = game.player()
local tris = game.collision(p.pos.x, p.pos.z, 4)
if tris then
  print(#tris, "triangles within 4 yalms")
  local t = tris[1]
  if t then print("first corner", t[1].x, t[1].y, t[1].z, "kind", t.kind) end
end
```

## `game.collision_mesh`

```lua
game.collision_mesh() -> string?
```

Every collision triangle of the zone at once, packed as binary, for exporting the zone or building a navigation mesh.

**Returns** `string?`: nil before the zone's collision has loaded. Otherwise 36 bytes per triangle: nine little-endian 32-bit floats, the corners' x, y, z one after another. Open doors are left out.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local mesh = game.collision_mesh()
if mesh then
  print(#mesh // 36, "triangles")
  local x1, y1, z1, x2, y2, z2, x3, y3, z3 = string.unpack("<fffffffff", mesh, 1)
  print("first triangle", x1, y1, z1, x2, y2, z2, x3, y3, z3)
end
```

## `game.raycast`

```lua
game.raycast(from, to) -> table?
```

Follows the straight line from one point to another and reports where it first hits the zone's collision. Use it for line of sight, or to find a wall ahead.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `from` | `{x, y, z}` | yes | The start point (world coordinates). |
| `to` | `{x, y, z}` | yes | The end point. |

**Returns** `table?`: the first hit as `{x, y, z, distance}`: the point hit and its distance from `from` in yalms. nil when the line hits nothing, when the two points are the same, or before the zone's collision has loaded.

**Errors** "game state is not available here" in the lobby or a chat filter; an error when a point lacks a numeric `x`, `y` or `z`.

**Example**

```lua
local p = game.player()
local h = p.heading
local from = { x = p.pos.x, y = p.pos.y - 1, z = p.pos.z }
local to = { x = from.x + math.cos(h) * 20, y = from.y, z = from.z - math.sin(h) * 20 }
local hit = game.raycast(from, to)
print(hit and ("Something %.1f yalms ahead"):format(hit.distance) or "Clear for 20 yalms")
```

A heading `h` points along x = cos(h), z = -sin(h). `y - 1` is one yalm above your feet, since y points down.

## `game.floor`

```lua
game.floor(x, z, y?) -> number?
```

The height of the floor a character at (x, y, z) would stand on: the first floor at or below that height (allowing half a yalm of sinking), else the surface over (x, z) nearest that height.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `x` | `number` | yes | World x. |
| `z` | `number` | yes | World z. |
| `y` | `number` | no, default your height | The height to look down from. |

**Returns** `number?`: the floor's y (remember: smaller y is higher). nil when there is no surface over (x, z), or before the zone's collision has loaded.

**Errors** "game state is not available here" in the lobby or a chat filter; an error when `x`, `z` or `y` is not a number.

**Example**

```lua
local p = game.player()
local y = game.floor(p.pos.x + 5, p.pos.z)
if y then
  print(("The ground 5 yalms east is %.2f yalms %s"):format(math.abs(y - p.pos.y), y < p.pos.y and "higher" or "lower"))
end
```

## `game.cast`

```lua
game.cast() -> table?
```

Your cast bar while it shows: what you are casting and how far along it is.

**Returns** `table?`: nil while no cast bar shows. Otherwise:

| Field | Type | Description |
| --- | --- | --- |
| `spell` | `integer?` | The spell id; nil for a bar an event opened. |
| `total` | `number` | The cast's whole length in seconds. |
| `left` | `number` | Seconds left. |
| `progress` | `number` | How far along, 0 to 1. |
| `event` | `boolean` | An event opened the bar (it then shows the time left). |

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
timer.every(0.5, function()
  local c = game.cast()
  if c and c.spell then
    local s = res.spell(c.spell)
    print(("%s: %.1f s left (%d%%)"):format(s and s.name or "?", c.left, math.floor(c.progress * 100)))
  end
end)
```

## `game.info`

```lua
game.info() -> table
```

Flags about the session and what is happening on screen.

**Returns** `table`, never nil:

| Field | Type | Description |
| --- | --- | --- |
| `zone` | `integer?` | The zone id, as `game.zone()`. |
| `server` | `string` | The server's (world's) name. |
| `mog_house` | `boolean` | You are in a Mog House. |
| `chat_open` | `boolean` | The chat line is open for typing. |
| `menu_open` | `boolean` | A game window or the battle menu is open (a mod window that has the keys counts). |
| `event` | `boolean` | An event (cutscene, NPC talk) runs. |
| `in_combat` | `boolean` | You are engaged (status 1). |
| `locked` | `boolean` | You are locked on to your target. |
| `autorun` | `boolean` | Autorun is on. |
| `follow` | `integer?` | The server id of who you follow; nil when you follow nobody. |
| `pet_index` | `integer?` | Your pet's index; nil without a pet. |
| `map_grid` | `string?` | Your map grid square as `<pos>` shows it, such as `"(F-7)"`, or `"(?-?)"` where the zone has no map; nil in a Mog House or before it is known. Updated twice a second. |
| `modern_ui` | `boolean` | The modern interface is on (false: the classic one). |

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local i = game.info()
print("Server:", i.server, "grid:", i.map_grid or "-")
if i.in_combat and not i.locked then
  print("Engaged, but not locked on")
end
```

## `game.camera`

```lua
game.camera() -> table
```

The camera as it is drawn this frame.

**Returns** `table`, never nil:

| Field | Type | Description |
| --- | --- | --- |
| `eye` | `{x, y, z}` | Where the camera is. |
| `look` | `{x, y, z}` | The point it looks at. |
| `distance` | `number` | The distance from `eye` to `look`, in yalms. |

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local c = game.camera()
print(("The camera is %.1f yalms from its look point, %.1f above it"):format(c.distance, c.look.y - c.eye.y))
```

## `game.shop`

```lua
game.shop() -> table[]?
```

The rows of the NPC shop you have open, to read prices or buy with `actions.buy`.

**Returns** `table[]?`: nil when no NPC shop is open (a guild shop counts as none). Otherwise a list of rows:

| Field | Type | Description |
| --- | --- | --- |
| `index` | `integer` | The row number, from 1: what `actions.buy` takes. |
| `item` | `integer` | The item id. |
| `price` | `integer` | Its price in gil. |

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
local rows = game.shop()
if rows then
  for _, r in ipairs(rows) do
    local it = res.item(r.item)
    print(r.index, it and it.name or r.item, r.price .. " gil")
  end
end
```

## `game.npc_names`

```lua
game.npc_names() -> table<integer, string>
```

The zone's list of NPC names by server id, including NPCs that are not spawned or not in range (the names Wide Scan shows).

**Returns** `table<integer, string>`: name by server id; never nil. It is replaced when a new zone loads.

**Errors** "game state is not available here" in the lobby or a chat filter.

**Example**

```lua
for id, name in pairs(game.npc_names()) do
  if name:find("Moogle") then
    print(id, name)
  end
end
```
