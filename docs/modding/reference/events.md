# `events`

Handlers for the client's events. `events.on` gives the client a function to call each time an event happens: when
HP changes, a zone loads, a chat line arrives, and so on (every event and its arguments is in the
[event list](#contents) below). Handlers run once a frame during the mod phase, each as its coroutine, so a
handler may `sleep` without holding up the game or other handlers. No permission is needed to watch events.

```lua
events.on("zone_change", function(zone, previous)
  print("Now in " .. (res.zone(zone) or zone))
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`events.on(event, handler, options?)`](#eventson) | Calls `handler` every time `event` happens; returns an id. |
| [`events.off(id)`](#eventsoff) | Removes a handler added by `events.on`. |

## `events.on`

```lua
events.on(event, handler, options?) -> integer
```

Registers `handler` for the event named `event`. The handler is called with the event's arguments each time the event
happens, until it is removed with `events.off` or the mod unloads.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `event` | `string` | yes | The event's name, from the [event list](#contents), or a name another mod sends with `mods.emit`. Any string is accepted: a name nothing raises never fires. |
| `handler` | `function` | yes | Called with the event's arguments. What it returns is ignored. |
| `options` | `table` | no | See below. |

**`options` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `priority` | `integer` | `0` | Handlers with a higher priority run first. Handlers with the same priority run in the order they were added. A value that is not an integer counts as 0. |

**Returns** `integer`: the handler's id, for `events.off`. Ids are unique within the mod.

**Example**

```lua
-- Runs before handlers with the default priority of 0.
events.on("hp_change", function(hp, previous)
  if hp < previous then
    print("Lost " .. (previous - hp) .. " HP")
  end
end, { priority = 10 })
```

Notes:

- Events are collected as they happen and delivered in the next mod phase, in the order they happened (`load`,
  `unload`, `logout` and `settings_changed` are delivered at once).
- Each call starts a new coroutine. A handler that sleeps does not delay the next handler; the next event starts a
  new call even if the last one is still sleeping.
- An event handler is not the player's input: a game action from it (for example `commands.run('/ma "Cure" <me>')`)
  needs the `automation` permission. `actions` is not enough.
- An error in a handler is printed in the chat log (once per distinct message). A handler that runs for more than
  100 ms without sleeping is stopped with "ran past its time budget". 20 errors within 60 seconds suspend the mod until
  `/lua reload`.

## `events.off`

```lua
events.off(id)
```

Removes the handler with this id. An id that matches no handler does nothing.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | What `events.on` returned. |

**Returns** nothing.

**Example**

```lua
-- A handler that runs only once.
local id
id = events.on("zone_change", function(zone)
  print("First zone change of the session: " .. zone)
  events.off(id)
end)
```

A call of the handler that is already sleeping is not stopped: it finishes.

# Events

Every event a mod can watch with `events.on(name, handler)`. The handler gets the arguments in the order listed.

Some things hold for all of them:

- Server ids (`actor`, `target`, `id` of an entity) are entity server ids: pass them to `game.entity`.
- The change events (`hp_change`, `buff_gain`, `item_add`, `time_change`...) compare the game's state with the
  previous frame's. A value that changes and changes back within one frame raises nothing, and there are none on the
  first frame after login (nothing to compare with yet).
- Event handlers are not the player's input: game actions from them need the `automation` permission.

## Contents

| Group | Events |
| --- | --- |
| [Lifecycle](#lifecycle) | [`load`](#load), [`unload`](#unload), [`login`](#login), [`logout`](#logout), [`lobby_screen`](#lobby_screen), [`frame`](#frame) |
| [Character](#character) | [`hp_change`](#hp_change), [`mp_change`](#mp_change), [`tp_change`](#tp_change), [`hpp_change`](#hpp_change), [`mpp_change`](#mpp_change), [`status_change`](#status_change), [`job_change`](#job_change), [`level_up`](#level_up), [`level_down`](#level_down), [`buff_gain`](#buff_gain), [`buff_lose`](#buff_lose), [`target_change`](#target_change) |
| [Party](#party) | [`party_change`](#party_change), [`party_buffs`](#party_buffs), [`party_invite`](#party_invite) |
| [Combat](#combat) | [`action`](#action), [`battle_message`](#battle_message), [`damage`](#damage), [`suggestions`](#suggestions) |
| [Chat](#chat) | [`chat_message`](#chat_message), [`emote`](#emote), [`examined`](#examined) |
| [Crafting](#crafting) | [`synth_amount`](#synth_amount), [`synth_result`](#synth_result) |
| [Items](#items) | [`item_add`](#item_add), [`item_remove`](#item_remove), [`item_change`](#item_change) |
| [World](#world) | [`zone_change`](#zone_change), [`zone_text`](#zone_text), [`time_change`](#time_change), [`day_change`](#day_change), [`moon_change`](#moon_change), [`weather_change`](#weather_change) |
| [Input and UI](#input-and-ui) | [`focus_change`](#focus_change), [`settings_changed`](#settings_changed) |
| [Network](#network) | [`ipc_message`](#ipc_message), [events from other mods](#events-from-other-mods) |

## Lifecycle

## `load`

Fires once, right after the mod's entry file has run: when the character enters the world (or, for a lobby mod, when
the lobby first shows), and after `/lua load` or `/lua reload`. Register the handler in the entry file.

No arguments.

```lua
events.on("load", function()
  print(mod.name .. " " .. mod.version .. " loaded")
end)
```

## `unload`

Fires just before the mod is unloaded: at logout, on `/lua unload`, on `/lua reload` (before the new copy loads), and
for a lobby mod when the client closes. `storage.account` and `storage.character` are saved right after it, so this is
the place for last changes to them. Do not sleep here: the mod is gone right after, and a sleeping handler never
resumes. Game state can be read, except in the lobby (a lobby mod unloading as the client closes, or reloaded there).

No arguments.

```lua
events.on("unload", function()
  storage.character.last_seen = os.time()
end)
```

## `login`

Fires once per login, when the character is in the world: once a zone is in and the server has sent our HP, MP and TP
(or 5 seconds after the zone, whichever comes first). The world's mods load at that moment, so it comes right after
their `load`.

| Argument | Type | Description |
| --- | --- | --- |
| `name` | `string` | The character's name. |

```lua
events.on("login", function(name)
  print("Welcome back, " .. name .. ".")
end)
```

## `logout`

Fires when the character leaves the world and the client goes back to the login screen: a logout, or a lost
connection. Every loaded mod gets it, lobby mods too. The other mods are unloaded right after (`unload` follows), so
do not sleep here. Game state can still be read.

No arguments.

```lua
events.on("logout", function()
  log.info("logged out at " .. os.date("%H:%M"))
end)
```

## `lobby_screen`

Fires when the screen before login changes. Lobby mods (`lobby = true` in mod.toml) get it from the first lobby screen
on; when the character enters the world, every mod gets it with `"world"`. See [lobby](lobby.md).

| Argument | Type | Description |
| --- | --- | --- |
| `screen` | `string` | The new screen: `login`, `connecting`, `title`, `select`, `create`, `delete`, `config` or `world`. |
| `previous` | `string?` | The screen before; nil for the first one a lobby mod sees. |

```lua
events.on("lobby_screen", function(screen, previous)
  log.info("lobby: " .. tostring(previous) .. " -> " .. screen)
end)
```

## `frame`

Fires every frame, after the frame's other events, timers and wake-ups (lobby mods get it in the lobby too). It costs
time on every frame, so prefer a `timer.every` or a specific event where one fits.

| Argument | Type | Description |
| --- | --- | --- |
| `dt` | `number` | Seconds since the previous frame. |

```lua
local elapsed = 0
events.on("frame", function(dt)
  elapsed = elapsed + dt
  if elapsed >= 1 then
    elapsed = 0
    hud.text("clock", { x = 10, y = 10, text = os.date("%H:%M:%S") })
  end
end)
```

## Character

## `hp_change`

Our HP changed (only once the server has sent our HP).

| Argument | Type | Description |
| --- | --- | --- |
| `hp` | `integer` | HP now. |
| `previous` | `integer` | HP before. |

```lua
events.on("hp_change", function(hp, previous)
  if hp == 0 and previous > 0 then print("Down!") end
end)
```

## `mp_change`

Our MP changed.

| Argument | Type | Description |
| --- | --- | --- |
| `mp` | `integer` | MP now. |
| `previous` | `integer` | MP before. |

```lua
events.on("mp_change", function(mp, previous)
  if mp < 50 and previous >= 50 then print("MP low") end
end)
```

## `tp_change`

Our TP changed.

| Argument | Type | Description |
| --- | --- | --- |
| `tp` | `integer` | TP now. |
| `previous` | `integer` | TP before. |

```lua
events.on("tp_change", function(tp, previous)
  if tp >= 1000 and previous < 1000 then print("Weapon skill ready") end
end)
```

## `hpp_change`

Our HP percentage changed.

| Argument | Type | Description |
| --- | --- | --- |
| `hpp` | `integer` | HP percent now (0..100). |
| `previous` | `integer` | HP percent before. |

```lua
events.on("hpp_change", function(hpp, previous)
  if hpp < 25 and previous >= 25 then chat.add("HP below 25%!", 200) end
end)
```

## `mpp_change`

Our MP percentage changed.

| Argument | Type | Description |
| --- | --- | --- |
| `mpp` | `integer` | MP percent now (0..100). |
| `previous` | `integer` | MP percent before. |

```lua
events.on("mpp_change", function(mpp)
  hud.bar("mp", { x = 10, y = 40, w = 120, h = 8, value = mpp / 100, kind = "mp" })
end)
```

## `status_change`

Our status changed: 0 idle, 1 engaged, 2 and 3 K.O., 5 mounted, 33 resting, and others.

| Argument | Type | Description |
| --- | --- | --- |
| `status` | `integer` | The status now. |
| `previous` | `integer` | The status before. |

```lua
events.on("status_change", function(status, previous)
  if status == 1 then print("Engaged") elseif previous == 1 then print("Disengaged") end
end)
```

## `job_change`

Our main job or support job changed. A level change alone raises `level_up` or `level_down` instead.

| Argument | Type | Description |
| --- | --- | --- |
| `main` | `integer` | Main job id (1 WAR .. 22 RUN; `res.job` names it). |
| `main_level` | `integer` | Main job level. |
| `sub` | `integer` | Support job id. |
| `sub_level` | `integer` | Support job level. |

```lua
events.on("job_change", function(main, main_level, sub, sub_level)
  print(("Now %s%d/%s%d"):format(res.job(main) or main, main_level, res.job(sub) or sub, sub_level))
end)
```

## `level_up`

Our main job's level went up (same jobs as before).

| Argument | Type | Description |
| --- | --- | --- |
| `level` | `integer` | The new level. |

```lua
events.on("level_up", function(level)
  print("Level " .. level .. "!")
end)
```

## `level_down`

Our main job's level went down (same jobs as before).

| Argument | Type | Description |
| --- | --- | --- |
| `level` | `integer` | The new level. |

```lua
events.on("level_down", function(level)
  print("Level down to " .. level)
end)
```

## `buff_gain`

We gained a status effect. With two copies of the same effect, each copy raises its event.

| Argument | Type | Description |
| --- | --- | --- |
| `id` | `integer` | The status effect's id (`res.status` names it). |

```lua
events.on("buff_gain", function(id)
  print("Gained " .. (res.status(id) or id))
end)
```

## `buff_lose`

We lost a status effect (one event per copy lost).

| Argument | Type | Description |
| --- | --- | --- |
| `id` | `integer` | The status effect's id. |

```lua
events.on("buff_lose", function(id)
  print((res.status(id) or id) .. " wore off")
end)
```

## `target_change`

Our target changed.

| Argument | Type | Description |
| --- | --- | --- |
| `id` | `integer?` | The new target's server id; nil when we have no target. |
| `previous` | `integer?` | The old target's server id; nil when we had none. |

```lua
events.on("target_change", function(id)
  local t = id and game.entity(id)
  if t then print("Target: " .. t.name) end
end)
```

## Party

## `party_change`

The members of our party or alliance changed: someone joined or left, or the order changed. Read `game.party()` for
the new list.

No arguments.

```lua
events.on("party_change", function()
  print("Party and alliance: " .. #game.party() .. " members")
end)
```

## `party_buffs`

The server sent our party's status effects (the other members' lists, as their party window icons show them). Read
`game.party()`: each member's `buffs` holds them. It comes whenever the server sends the lists, also when nothing
changed.

No arguments.

```lua
events.on("party_buffs", function()
  for _, m in ipairs(game.party()) do
    if m.slot > 0 then print(m.name, #m.buffs .. " effects") end
  end
end)
```

## `party_invite`

Someone invited us to a party or an alliance.

| Argument | Type | Description |
| --- | --- | --- |
| `from` | `string` | The inviting player's name. |
| `kind` | `string` | `"party"` or `"alliance"`. |

```lua
events.on("party_invite", function(from, kind)
  print(from .. " invited you to their " .. kind .. ".")
end)
```

## Combat

## `action`

An action arrived from the server: a round of melee hits, a ranged attack, a spell, a weapon skill, an ability, an
item, a monster's move, each start and finish.

| Argument | Type | Description |
| --- | --- | --- |
| `action` | [Action](types.md#action) | The action: actor, category, param, info and targets with their results. Categories include 1 melee, 2 ranged attack finished, 3 weapon skill finished, 4 spell finished, 5 item finished, 6 ability finished, 7 weapon skill started, 8 spell started, 9 item started, 10 ability started, 11 monster move finished, 12 ranged attack started. |

```lua
events.on("action", function(a)
  if a.category == 4 and a.actor == game.player().id then
    local s = res.spell(a.param)
    print("Finished casting " .. (s and s.name or a.param))
  end
end)
```

## `battle_message`

A battle message arrived by itself (not as part of an action). `res.message` turns it into the log's text.

| Argument | Type | Description |
| --- | --- | --- |
| `actor` | `integer` | The actor's server id. |
| `target` | `integer` | The target's server id. |
| `message` | `integer` | The message id. |
| `param1` | `integer` | The message's first value. |
| `param2` | `integer` | The message's second value. |

```lua
events.on("battle_message", function(actor, target, message, p1, p2)
  local m = res.message(message, { actor = actor, target = target, values = { p1, p2 } })
  if m then log.info(m.text) end
end)
```

## `damage`

An action result put a damage number up (a main result or its additional effect). It fires when the result's log line
is printed, which can be a moment after the `action` event (the line waits for the attacker's swing), and it fires
even when the line itself is hidden by the game's chat filter settings or by a mod's chat filter.

| Argument | Type | Description |
| --- | --- | --- |
| `actor` | `integer` | Who dealt it (server id). |
| `target` | `integer` | Who took it (server id). |
| `value` | `integer` | The number. |
| `message` | `integer` | The message id of the result's line. |

```lua
local total = 0
events.on("damage", function(actor, target, value)
  if actor == game.player().id then
    total = total + value
  end
end)
```

## `suggestions`

The Battle Suggestions mod's rules picked new rows for the modern battle menu: a spell, ability or weapon skill worth
using now, and why. The mod sends it (`mods.emit`) while the modern battle menu is open; when the menu closes an empty
list comes. While the player's cursor is on one of the rows they hold still, and only their submenus' spells are taken
again (no event then).

| Argument | Type | Description |
| --- | --- | --- |
| `rows` | `table[]` | Rows as the `battle_rows` [slot](ui.native.md#uinativeslot) takes them: `{kind, id, target, reason, label, group}`. |

```lua
-- Log what the Battle Suggestions mod picks.
events.on("suggestions", function(rows)
  for _, r in ipairs(rows) do print(r.kind, r.id, r.reason) end
end)
```

## Chat

## `chat_message`

A chat line arrived from the server: a player's say, shout, tell, party or linkshell line, a GM's line, and so on.
Lines from players on the block list do not raise it.

| Argument | Type | Description |
| --- | --- | --- |
| `text` | `string` | The message text. Auto-translate phrases are in it as special characters; `text.plain` turns them into words. |
| `sender` | `string` | The sender's name (may be empty for lines with no sender). |
| `kind` | `integer` | The kind of line: 0 say, 1 shout, 3 tell, 4 party, 5 linkshell, 26 yell, 27 linkshell 2, 33 unity, and others. |
| `gm` | `boolean` | Whether a GM sent it. |

```lua
events.on("chat_message", function(msg, sender, kind, gm)
  if kind == 3 then
    print("Tell from " .. sender .. ": " .. text.plain(msg))
  end
end)
```

## `emote`

Someone used an emote. It does not fire for a player on your block list, as the game shows neither the motion nor the line.

| Argument | Type | Description |
| --- | --- | --- |
| `actor` | `integer` | Who used it (server id). |
| `target` | `integer` | At whom (server id); 0 for none. |
| `emote` | `integer` | The emote's id. |
| `mode` | `integer` | 0 motion and log text, 1 text only (no motion), 2 or more motion only (no text). |

```lua
events.on("emote", function(actor, target, emote, mode)
  local me = game.player()
  if target == me.id and actor ~= me.id then
    local who = game.entity(actor)
    print((who and who.name or "Someone") .. " used an emote on you")
  end
end)
```

## `examined`

Someone examined us with `/check` (the "X examines you." message).

| Argument | Type | Description |
| --- | --- | --- |
| `id` | `integer` | The examiner's server id. |
| `name` | `string` | The examiner's name; empty when they are not in range. |

```lua
events.on("examined", function(id, name)
  print((name ~= "" and name or "Someone") .. " checked you.")
end)
```

## Crafting

## `synth_amount`

The synthesis window's count step was answered (it asks how many after Synthesize while the client option
`synth_amount` is on). The first synthesis has already gone out from the window.

| Argument | Type | Description |
| --- | --- | --- |
| `count` | `integer` | How many the player asked for, 1 or more. |
| `recipe` | `table` | `{crystal, items}`: the crystal's item id and one material id per unit, as `actions.synth` takes them. |

```lua
events.on("synth_amount", function(count, recipe)
  print(("%d to go"):format(count - 1))
end)
```

## `synth_result`

Our synthesis was answered (s2c 0x06F). The game has printed its lines.

| Argument | Type | Description |
| --- | --- | --- |
| `result` | `integer` | 0 and 12 made, 1 failed (materials lost); anything else: refused or cut short. |
| `item` | `integer` | The item made. |
| `count` | `integer` | How many were made. |
| `grade` | `integer` | The quality: 0 normal, 1 to 3 high quality. |

```lua
events.on("synth_result", function(result, item, count)
  if result == 0 or result == 12 then print(("made %d x %s"):format(count, res.item(item).name)) end
end)
```

## Items

The item events are watched only while some loaded mod has a handler for one of them (the bags are large to
compare). They start a frame or two after the first such handler is added. `bag` and `slot` are the game's numbers, as
`game.items` gives them (bag 0 is the inventory).

## `item_add`

An item appeared in a bag slot that was empty, or a slot now holds a different item (then `item_remove` for the old
one comes first).

| Argument | Type | Description |
| --- | --- | --- |
| `bag` | `integer` | The bag (0 inventory, 1 safe, 2 storage...). |
| `slot` | `integer` | The slot in the bag. |
| `id` | `integer` | The item id (`res.item` describes it). |
| `count` | `integer` | How many are in the slot. |

```lua
events.on("item_add", function(bag, slot, id, count)
  if bag == 0 then
    local item = res.item(id)
    print("Got " .. count .. " x " .. (item and item.name or id))
  end
end)
```

## `item_remove`

An item left a bag slot (the slot is empty now, or holds a different item).

| Argument | Type | Description |
| --- | --- | --- |
| `bag` | `integer` | The bag. |
| `slot` | `integer` | The slot in the bag. |
| `id` | `integer` | The item that was there. |
| `count` | `integer` | How many there were. |

```lua
events.on("item_remove", function(bag, slot, id, count)
  log.info(("bag %d slot %d: %d x item %d gone"):format(bag, slot, count, id))
end)
```

## `item_change`

The count of the same item in a bag slot changed (a stack used, sold or added to).

| Argument | Type | Description |
| --- | --- | --- |
| `bag` | `integer` | The bag. |
| `slot` | `integer` | The slot in the bag. |
| `id` | `integer` | The item id. |
| `count` | `integer` | How many now. |
| `previous` | `integer` | How many before. |

```lua
events.on("item_change", function(bag, slot, id, count, previous)
  if count < previous then
    print("Used " .. (previous - count) .. " of item " .. id)
  end
end)
```

## World

## `zone_change`

A new zone has loaded (its scenery may still be appearing). It does not fire for the first zone after login: `login`
fires then.

| Argument | Type | Description |
| --- | --- | --- |
| `zone` | `integer` | The new zone's id (`res.zone` names it). |
| `previous` | `integer?` | The zone we came from. |

```lua
events.on("zone_change", function(zone, previous)
  print("Entered " .. (res.zone(zone) or zone))
end)
```


## `zone_text`

A zone dialogue line came from the server (s2c 0x036, 0x02A or 0x027): an NPC's or an event's line printed in the
log. The id is the entry's in the installed game files (the server's id mapped to it), for
[`res.zone_text`](res.md#reszone_text).

| Argument | Type | Description |
| --- | --- | --- |
| `id` | `integer` | The dialogue entry's id in this zone's file. |
| `actor` | `integer` | The server id the line is about (the NPC, or us). |
| `plain` | `boolean` | The server marked it plain (MesNum bit 15): printed without the speaker's name. |
| `packet` | `integer` | The packet it came in: `0x036`, `0x02A` or `0x027`. |

```lua
events.on("zone_text", function(id, actor, plain)
  log.info(("zone text %d about %d"):format(id, actor))
end)
```

## `time_change`

A Vana'diel minute passed.

| Argument | Type | Description |
| --- | --- | --- |
| `minute` | `integer` | Minutes since Vana'diel midnight (hour * 60 + minute). |
| `previous` | `integer` | The minute before. |

```lua
events.on("time_change", function(minute)
  if minute == 6 * 60 then print("Dawn in Vana'diel.") end
end)
```

## `day_change`

A new Vana'diel day began.

| Argument | Type | Description |
| --- | --- | --- |
| `weekday` | `integer` | The weekday (0 Firesday; `res.day` names it). |
| `previous` | `integer` | The weekday before. |

```lua
events.on("day_change", function(weekday)
  print("Today is " .. (res.day(weekday) or weekday))
end)
```

## `moon_change`

The moon's phase changed.

| Argument | Type | Description |
| --- | --- | --- |
| `phase` | `integer` | The phase now (`res.moon` names it). |
| `previous` | `integer` | The phase before. |

```lua
events.on("moon_change", function(phase)
  print("The moon: " .. (res.moon(phase) or phase))
end)
```

## `weather_change`

The weather changed.

| Argument | Type | Description |
| --- | --- | --- |
| `weather` | `integer` | The weather id now (`res.weather` names it). |
| `previous` | `integer` | The weather before. |

```lua
events.on("weather_change", function(weather)
  print("Weather: " .. (res.weather(weather) or weather))
end)
```

## Input and UI

## `focus_change`

The game window gained or lost the keyboard focus (in the world only).

| Argument | Type | Description |
| --- | --- | --- |
| `focused` | `boolean` | Whether the window has the focus now. |

```lua
events.on("focus_change", function(focused)
  audio.mute_music(not focused)
end)
```

## `settings_changed`

The player changed one of this mod's settings, on its Extras page or with `/lua set <mod> <key> <value>`. It is
delivered right away, not in the next mod phase. Only the mod whose setting changed gets it.

| Argument | Type | Description |
| --- | --- | --- |
| `key` | `string` | The setting's key, as declared in mod.toml. |
| `value` | `any` | The new value (a boolean, number, string or table, as the setting's type). |

```lua
events.on("settings_changed", function(key, value)
  print("Setting " .. key .. " is now " .. tostring(value))
end)
```

## Network

## `ipc_message`

The same mod, running in another game window on this computer, sent text with `ipc.send`. The mod needs the
`multibox` permission (granted) to get it. A message sent to one character reaches only the window playing that
character.

| Argument | Type | Description |
| --- | --- | --- |
| `text` | `string` | The text sent. |
| `from` | `string` | The name of the character in the sending window. |

```lua
events.on("ipc_message", function(msg, from)
  if msg == "follow me" then
    print(from .. " asks you to follow")
  end
end)
```

## Events from other mods

`mods.emit(name, ...)` raises an event of any name in every other loaded mod; `events.on(name, handler)` receives it
with the values sent (copies of plain data). Names should start with the sending mod's id so they never clash with the
client's events. See [mods.emit](mods.md#modsemit).

```lua
events.on("tally_kill", function(info)
  print(info.name .. " defeated")
end)
```
