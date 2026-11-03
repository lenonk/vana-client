# `actions`

Do things in the game the way the player does: cast spells, use abilities and items, change gear, target, move, talk to NPCs and answer their menus, run macros and manage the party. Each call goes through the same checks, refusal lines and server requests as the game's menus, keys and text commands, so whatever the game would refuse is refused the same way, with its usual line in the chat log (a refusal is not a Lua error). Every function needs one of two permissions: `actions` lets the mod act only while it handles the player's input (one of its commands typed by the player or run from a macro or key bind, but not by a mod's `commands.run`; a `ui.on` window event, a `hud.on` click, a `binds.set` function, an `input.on_key` / `input.on_mouse` filter, `/lua run`), and a handler that started that way keeps the right through `sleep` and `wait_until`; `automation` lets it act at any time, from `events.on` handlers and timers too. Without them a call raises `game actions need the actions or automation permission: list it in mod.toml, then /lua grant <mod> actions`, or, with `actions` outside the player's input, `a game action outside the player's input needs the automation permission` (the **Errors** lines below list only the other errors). Most calls are queued and carried out right after the mod phase of the same frame (in the lobby they are dropped), so the game state you read back in the same handler is still the old one and the server's answer comes frames later: wait for it with `sleep` or `wait_until`; `actions.trade`, `actions.buy` and `actions.sell` send at once and raise their refusals as errors.

```lua
-- mod.toml: permissions = ["actions", "automation"]
-- //cure [target]: Cure (spell 1) on a name or tag, else on the current target.
commands.register("cure", function(who)
  actions.cast(1, who or "<t>")
end)

-- automation: rest a moment after each fight ends.
events.on("status_change", function(status, previous)
  if previous == 1 and status == 0 then
    sleep(2)
    actions.heal(true)
  end
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`actions.cast(spell, target?)`](#actionscast) | Casts a spell, with the magic menu's checks. |
| [`actions.ability(id, target?)`](#actionsability) | Uses a job ability, weapon skill, pet command or monster skill, with the ability menu's checks. |
| [`actions.weaponskill(id, target?)`](#actionsweaponskill) | The same function as `actions.ability`. |
| [`actions.ranged(target?)`](#actionsranged) | A ranged attack (`/range`). |
| [`actions.attack(target?)`](#actionsattack) | Engages a monster, or switches to it while engaged. |
| [`actions.disengage()`](#actionsdisengage) | Disengages (`/attack off`). |
| [`actions.assist(target?)`](#actionsassist) | Takes a player's target (`/assist`). |
| [`actions.heal(on?)`](#actionsheal) | Starts or stops resting (`/heal`). |
| [`actions.target(id?)`](#actionstarget) | Sets or clears the current target. |
| [`actions.lock(on)`](#actionslock) | Locks on to the current target, or lets go. |
| [`actions.use_item(item, target?)`](#actionsuse_item) | Uses an item by its id (`/item`). |
| [`actions.use_slot(bag, index, target?)`](#actionsuse_slot) | Uses the item in one bag slot. |
| [`actions.move(bag, index, to, count?)`](#actionsmove) | Moves an item to another bag. |
| [`actions.sort(bag)`](#actionssort) | Sorts a bag and merges its stacks. |
| [`actions.drop(index)`](#actionsdrop) | Drops an inventory stack, without asking. |
| [`actions.equip(slot, piece)`](#actionsequip) | Wears one piece, or a whole table of them. |
| [`actions.unequip(slot)`](#actionsunequip) | Takes off what a slot wears. |
| [`actions.equip_set(n)`](#actionsequip_set) | Wears one of the game's equip sets. |
| [`actions.lot(slot)`](#actionslot) | Lots on a treasure pool item. |
| [`actions.pass(slot)`](#actionspass) | Passes on a treasure pool item. |
| [`actions.cancel(status)`](#actionscancel) | Cancels one of your status effects. |
| [`actions.talk(id)`](#actionstalk) | Talks to an NPC. |
| [`actions.choose(option)`](#actionschoose) | Picks an option of the event menu showing. |
| [`actions.advance()`](#actionsadvance) | Moves the event's waiting text on, as Enter does. |
| [`actions.trade(npc, items?, gil?)`](#actionstrade) | Hands items and gil to an NPC. |
| [`actions.buy(row, count?)`](#actionsbuy) | Buys from the open NPC shop. |
| [`actions.sell(slot, count?)`](#actionssell) | Sells an inventory item to the open NPC shop. |
| [`actions.blue_set(slot, spell)`](#actionsblue_set) | Sets a blue magic slot. |
| [`actions.blue_remove(slot)`](#actionsblue_remove) | Empties a blue magic slot. |
| [`actions.attach(slot, item)`](#actionsattach) | Sets an automaton slot. |
| [`actions.detach(slot)`](#actionsdetach) | Empties an automaton slot. |
| [`actions.invite(name)`](#actionsinvite) | Invites a player to the party. |
| [`actions.party_leave()`](#actionsparty_leave) | Leaves the party. |
| [`actions.party_kick(name)`](#actionsparty_kick) | Removes a member from the party. |
| [`actions.join()`](#actionsjoin) | Accepts a party invitation. |
| [`actions.decline()`](#actionsdecline) | Declines a party invitation. |
| [`actions.mount(name)`](#actionsmount) | Calls a mount by name. |
| [`actions.dismount()`](#actionsdismount) | Gets off the mount. |
| [`actions.open_menu(name)`](#actionsopen_menu) | Opens a game window as its main menu row does. |
| [`actions.macro(row, n)`](#actionsmacro) | Runs a macro of the current set. |
| [`actions.macro_set(book, set)`](#actionsmacro_set) | Switches the macro palette's book and set. |
| [`actions.macro_edit(row, n, line, text)`](#actionsmacro_edit) | Changes a macro's line or title, and saves it. |
| [`actions.scan()`](#actionsscan) | Asks for the Wide Scan list. |
| [`actions.track(index)`](#actionstrack) | Tracks an entity with Wide Scan. |
| [`actions.untrack()`](#actionsuntrack) | Stops Wide Scan tracking. |
| [`actions.run(heading?)`](#actionsrun) | Runs on (autorun) in a direction. |
| [`actions.run_to(x, z, within?)`](#actionsrun_to) | Runs to a point and stops there. |
| [`actions.follow(id)`](#actionsfollow) | Follows an entity (`/follow`). |
| [`actions.walk(on)`](#actionswalk) | Switches between walking and running. |
| [`actions.stop()`](#actionsstop) | Stops a run, a run to a point or a follow. |
| [`actions.turn(heading)`](#actionsturn) | Turns to face a direction. |

## `actions.cast`

```lua
actions.cast(spell, target?) -> nil
```

Casts a spell by its id, as picking it in the magic menu does: the menu's checks first, then the spell goes out as a `/magic` command line on the target. Use it for click-to-cast buttons, healing commands and automated buffs.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `spell` | `integer` | yes | The spell id (as in `res.spell` and `game.spells`). |
| `target` | `integer\|string` | no | Who to cast on: a server id, a target tag such as `"<t>"` or `"<me>"`, or a name. Left out, a spell that can only target you goes on you; any other goes on your current target, or on you when nothing is targeted and you can be its target. Every form is listed in the notes. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Errors** `a target is a server id or a tag / name string` when `target` is not an integer, a string or nil.

**Example**

```lua
-- //castid <spell id> [target]
commands.register("castid", function(id, who)
  local spell = tonumber(id)
  if spell then
    actions.cast(spell, who)   -- no target given: the spell's usual one
  end
end)
```

The checks, in order, when the action runs:

1. The magic menu's checks, each with its refusal line and the error sound: you are fishing, mounted, resting, sitting or in a Mog House; the spell is still recasting; your jobs cannot cast it (or the server has not sent your recast timers since login yet); not enough MP; a blue magic spell that is not set, or your blue magic is being reset; an Unbridled spell without Unbridled Learning or Unbridled Wisdom on.
2. The text command's checks, shared by every action that runs a command line: refused with the game's line while you are K.O. (unless the `dead_commands` option of `client.set_option` is on), mounted, in an event, charmed or fishing. A spell you have not learned, a spell name the game does not know, or no valid target prints `...A command error occurred.`

Target forms, as the game's commands read them (the target is looked up when the action runs, not when you call):

| Form | Means |
| --- | --- |
| nil | You, for a spell or ability that can only target you; otherwise your current target, or you when nothing is targeted and you can be its target. |
| an integer | A server id (`entity.id`) of anything the client knows in the zone (players, NPCs, monsters), or your own. |
| `"<t>"` | Your current target. |
| `"<me>"` | Yourself. |
| `"<bt>"` | The living monster with the lowest index that you or a party member has claimed. |
| `"<pet>"` | Your pet. |
| `"<ht>"` | The nearest living monster someone called for help on. |
| `"<scan>"` | The entity you are tracking with Wide Scan. |
| `"<p0>"` .. `"<p5>"`, `"<a10>"` .. `"<a15>"`, `"<a20>"` .. `"<a25>"`, `"<focust>"` | The party / alliance slot's member (or the focus target), by name. |
| `"<st>"`, `"<stpc>"`, `"<stnpc>"`, `"<stpt>"`, `"<stal>"` | Opens the game's target cursor (any, players, NPCs, party, alliance); the spell goes off when the player picks. Refused in an event or a Mog House. |
| a name | A player in the zone near you (a name of 3 to 15 letters matches in any case), or your own name; for `cast` and `ability` also your pet or a trust in your party. |

The command line also passes through every mod's `filters.typed` filter, as a typed line does.

## `actions.ability`

```lua
actions.ability(id, target?) -> nil
```

Uses an ability by its record id, as picking it in the ability menu does: the menu's checks, then the `/jobability`, `/weaponskill` or `/monsterskill` line for it. Job abilities, weapon skills, pet commands and (in Monstrosity) monster skills all go through here.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The ability's record id, as `res.ability` and `game.abilities` give it: weapon skills 0..511, job abilities 512..1023, pet commands 1024..1535. |
| `target` | `integer\|string` | no | As for [`actions.cast`](#actionscast): left out, an ability that only targets you (Berserk) goes on you, any other on your current target. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Errors** `a target is a server id or a tag / name string` when `target` is not an integer, a string or nil.

**Example**

```lua
-- //useability <ability name>: an ability you can use now, on its usual target.
commands.register("useability", function(...)
  local want = table.concat({...}, " ")
  for _, id in ipairs(game.abilities()) do
    local a = res.ability(id)
    if a and a.name == want then
      actions.ability(id)
      return
    end
  end
end)
```

An id that is no ability record, or a category row (a list such as a Blood Pact group), does nothing. The menu's checks, each with its refusal line and the error sound: a job trait ("Job traits are always active."); fishing, mounted, resting, sitting or in a Mog House; the ability is recasting; not enough TP or MP; a weapon skill while not engaged or below 1000 TP. Then the command line's checks as in [`actions.cast`](#actionscast); a job ability your jobs do not have now prints `...A command error occurred.`

## `actions.weaponskill`

```lua
actions.weaponskill(id, target?) -> nil
```

The same function as [`actions.ability`](#actionsability), under a name that reads better for weapon skills (their record ids are 0..511).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The weapon skill's record id (`res.ability`). |
| `target` | `integer\|string` | no, but see [`actions.cast`](#actionscast) | As for `actions.cast`; usually `"<t>"`. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Errors** `a target is a server id or a tag / name string` when `target` is not an integer, a string or nil.

**Example**

```lua
-- automation: a weapon skill as soon as TP reaches 1000.
local WS = 32 -- the weapon skill's record id (res.ability)
events.on("tp_change", function(tp, previous)
  if tp >= 1000 and previous < 1000 and game.player().status == 1 then
    actions.weaponskill(WS, "<t>")
  end
end)
```

## `actions.ranged`

```lua
actions.ranged(target?) -> nil
```

A ranged attack on a target, as `/range` does.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `target` | `integer\|string` | no, default `"<t>"` | A target as for [`actions.cast`](#actionscast), except that names match players only. Left out, your current target. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Errors** `a target is a server id or a tag / name string` when `target` is not an integer, a string or nil.

**Example**

```lua
commands.register("fire", function()
  actions.ranged()
end)
```

The command line's checks of [`actions.cast`](#actionscast) apply (K.O., mounted, event, charmed, fishing); no valid target prints `...A command error occurred.` Range, ammunition and the rest are the server's to check.

## `actions.attack`

```lua
actions.attack(target?) -> nil
```

Engages a monster, as `/attack` does; while engaged, it switches your attack to the new monster. The monster also becomes your current target.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `target` | `integer\|string` | no, default your current target | A server id or a target tag as for [`actions.cast`](#actionscast) (names match players only, who cannot be attacked). |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Errors** `a target is a server id or a tag / name string` when `target` is not an integer, a string or nil.

**Example**

```lua
-- //pull: engage the nearest monster within 20 yalms.
commands.register("pull", function()
  local best
  for _, e in ipairs(game.entities({ distance = 20, kinds = { "mob" } })) do
    if e.hpp > 0 and (not best or e.distance < best.distance) then
      best = e
    end
  end
  if best then actions.attack(best.id) end
end)
```

Without a target it engages your current target (nothing when you have none). With a target, it must be a living monster, else the game's refusal line; an id or a name that matches nothing in the zone raises an error Refused with the game's line while synthesizing; the command line's checks of `actions.cast` apply, but attacking while mounted is allowed.

## `actions.disengage`

```lua
actions.disengage() -> nil
```

Stops attacking, as `/attack off` does.

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- automation: stop attacking when HP falls under 25%.
events.on("hpp_change", function(hpp)
  if hpp < 25 and game.player().status == 1 then
    actions.disengage()
  end
end)
```

Refused with the game's line while synthesizing, K.O., in an event, charmed or fishing.

## `actions.assist`

```lua
actions.assist(target?) -> nil
```

Asks the server for another player's target, which then becomes yours, as `/assist` does.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `target` | `integer\|string` | no, default your current target | The player to assist: a name, a server id or a tag such as `"<p1>"` (see [`actions.cast`](#actionscast); names match players only). |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Errors** `a target is a server id or a tag / name string` when `target` is not an integer, a string or nil.

**Example**

```lua
-- //helpout <name>: take a party member's target.
commands.register("helpout", function(name)
  if name then actions.assist(name) end
end)
```

Does nothing while you are engaged. An id or a name that matches nothing in the zone raises an error. The command line's checks of `actions.cast` apply, but it is allowed while mounted.

## `actions.heal`

```lua
actions.heal(on?) -> nil
```

Starts or stops resting, as `/heal` does.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `on` | `boolean` | no, default toggles | `true` starts resting, `false` stops, nil switches between them. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
commands.register("rest", function()
  actions.heal()
end)
```

Works only while you stand idle or already rest; otherwise nothing happens. Refused with the game's line while sitting, K.O., mounted, in an event, charmed or fishing. In Monstrosity it does nothing unless you are already resting.

## `actions.target`

```lua
actions.target(id?) -> nil
```

Makes an entity your current target, or clears the target, as picking one with the keys or the mouse does (with the targeting sound).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | no, default clears the target | The entity's server id (`entity.id`). Names and tags are not taken here. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- //tnear: target the nearest NPC.
commands.register("tnear", function()
  local best
  for _, e in ipairs(game.entities({ kinds = { "npc" } })) do
    if not best or e.distance < best.distance then best = e end
  end
  if best then actions.target(best.id) end
end)
```

A new target ends a lock-on. The game's usual target checks still run every frame and drop a target that is not here, hidden, more than 50 yalms away, or a dead or despawning monster. It works while engaged too, but it does not switch the attack: use [`actions.attack`](#actionsattack) for that.

## `actions.lock`

```lua
actions.lock(on) -> nil
```

Locks on to the current target (`true`) or lets go (`false`), as the lock-on key does.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `on` | `boolean` | yes | `true` to lock on, `false` to release. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
commands.register("lockt", function()
  actions.lock(true)
end)
```

Locking on needs a current target that is not you and is in the zone, and you not K.O.; otherwise nothing happens. Changing the target lets go of the lock.

## `actions.use_item`

```lua
actions.use_item(item, target?) -> nil
```

Uses an item by its id, as `/item` does: the game finds the first item of that name in the temporary items, then the inventory, then the wardrobes.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `item` | `integer` | yes | The item id (as in `res.item` and `game.items`). |
| `target` | `integer\|string` | no | A target as for [`actions.cast`](#actionscast), except that names match players only. Left out, it follows the item's valid targets as `cast` does: food and medicine go on you. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Errors** `no item <id>` when the id has no item record (raised at the call); `game state is not available here (in the lobby, or outside the mod phase)` in the lobby; `a target is a server id or a tag / name string` when `target` is not an integer, a string or nil.

**Example**

```lua
local POTION = 4112
commands.register("potion", function()
  actions.use_item(POTION, "<me>")
end)
```

Refused with the game's line while mounted, fishing, synthesizing, resting, sitting or in a Mog House, and by the command line's checks of `actions.cast`. No item of that name in those bags, or no valid target, prints `...A command error occurred.` An item with charges must be worn and ready to use, else nothing happens. An item without charges used on an NPC (not a monster) is handed to it as a trade, from the inventory only and within 6 yalms ("Target out of range." beyond). In Monstrosity it does nothing.

## `actions.use_slot`

```lua
actions.use_slot(bag, index, target?) -> nil
```

Uses the item in one bag slot, as the item menu does. Use it when you need a particular copy of an item rather than the first one `/item` finds.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `bag` | `integer` | yes | The bag number, as in `game.items` (0 inventory, 3 temporary items, 8 and 10..16 the wardrobes). |
| `index` | `integer` | yes | The slot in that bag (an [Item](types.md#item)'s `slot`). |
| `target` | `integer` | no, default yourself | The target's server id. Names and tags are not taken here. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- //usefirst <item id>: use the first stack of it in the inventory on yourself.
commands.register("usefirst", function(id)
  for _, it in ipairs(game.items(0)) do
    if it.id == tonumber(id) then
      actions.use_slot(it.bag, it.slot)
      return
    end
  end
end)
```

Refused with the game's line while mounted, fishing, synthesizing, resting, sitting or in a Mog House. An empty slot does nothing. Items with charges and items used on an NPC behave as in [`actions.use_item`](#actionsuse_item).

## `actions.move`

```lua
actions.move(bag, index, to, count?) -> nil
```

Moves an item from a bag slot into the first free slot of another bag, as the storage windows do.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `bag` | `integer` | yes | The bag it is in (as in `game.items`). |
| `index` | `integer` | yes | Its slot in that bag. |
| `to` | `integer` | yes | The bag to move it to. |
| `count` | `integer` | no, default the whole stack | How many to move. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- //stash <item id>: every stack of it from the inventory into the satchel (bag 5).
commands.register("stash", function(id)
  for _, it in ipairs(game.items(0)) do
    if it.id == tonumber(id) then
      actions.move(0, it.slot, 5)
    end
  end
end)
```

Nothing happens when the slot is empty by the time the action runs. Whether the other bag is open to you and has room is the server's to decide. The default count is the stack as it was when you called.

## `actions.sort`

```lua
actions.sort(bag) -> nil
```

Sorts a bag's list, in the order the game's item sort setting gives, and asks the server to merge its partial stacks, as the item list's sort does.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `bag` | `integer` | yes | The bag number (as in `game.items`). |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
commands.register("tidy", function()
  actions.sort(0)
end)
```

## `actions.drop`

```lua
actions.drop(index) -> nil
```

Drops the whole stack in an inventory slot, at once.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `index` | `integer` | yes | The inventory slot (an [Item](types.md#item)'s `slot` in bag 0). |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- //junk <item id>: drop every stack of it from the inventory.
commands.register("junk", function(id)
  for _, it in ipairs(game.items(0)) do
    if it.id == tonumber(id) then actions.drop(it.slot) end
  end
end)
```

There is no confirmation, not even for the Rare, Ex, augmented and linkshell items the item menu asks about. An empty slot does nothing.

## `actions.equip`

```lua
actions.equip(slot, piece) -> nil
actions.equip(set) -> nil
```

Wears gear as the equipment window does: one piece into one slot, or a table of slots and pieces at once. A piece already worn in its slot stays on.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `slot` | `integer\|string` | yes, in the first form | An equipment slot: a number 0..15 or a name (table below). |
| `piece` | `integer\|table\|false` | yes, in the first form | What to wear there (table below). It cannot be nil: pass `false` to take the slot off. |
| `set` | `table` | yes, in the second form | Slots as keys, pieces as values: `{main = 16535, head = {bag = 8, index = 3}, ring2 = false}`. |

Slots:

| Number | Names |
| --- | --- |
| 0 | `main` |
| 1 | `sub` |
| 2 | `range` |
| 3 | `ammo` |
| 4 | `head` |
| 5 | `body` |
| 6 | `hands` |
| 7 | `legs` |
| 8 | `feet` |
| 9 | `neck` |
| 10 | `waist` |
| 11 | `left_ear`, `ear1` |
| 12 | `right_ear`, `ear2` |
| 13 | `left_ring`, `ring1` |
| 14 | `right_ring`, `ring2` |
| 15 | `back` |

Names are read in any case.

Pieces:

| Form | Means |
| --- | --- |
| an item id (`integer`) | The piece already in that slot if it is this item; else the first copy in the inventory, then wardrobes 8 and 10..16, that no other slot wears. |
| `{bag = , index = }` | That exact piece: its bag number and slot (from `game.items`). |
| `false` or `0` | Take off what the slot wears. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Errors** `no equipment slot "<name>"` for an unknown slot name; `an equipment slot is 0..15 or a name (main, sub, range, ammo, head, ...)` for another slot value; `a piece is an item id, {bag = , index = }, or false` for another piece value; `actions.equip(slot, item) or actions.equip({slot = item, ...})` when called with one argument that is not a table (or with nil as the piece).

**Example**

```lua
-- Your gear's item ids.
local TP = { main = 16535, head = 12448, ring1 = false }
commands.register("tp", function()
  actions.equip(TP)
end)
commands.register("nohat", function()
  actions.equip("head", false)
end)
```

Refused with the game's line while fishing or synthesizing. Each piece is then tried as the equipment window tries it: a slot your job's encumbrance locks, a piece you cannot wear now (race, main job, level), a piece that does not fit the slot, or a wardrobe that is not open to you is skipped without a line. A weapon for the sub slot is refused with the game's line unless your main or sub job is Thief, Ninja, Blue Mage or Dancer, when no main weapon is worn, or when the main weapon is two-handed. The pieces of a table go in the order `pairs` gives (no fixed order). The game learns what you wear only when the server answers, so two slots given the same item id in one call (a pair of rings or earrings) both pick the same copy: name the second copy as `{bag = , index = }`.

## `actions.unequip`

```lua
actions.unequip(slot) -> nil
```

Takes off what a slot wears, as picking the worn piece again in the equipment window does.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `slot` | `integer\|string` | yes | An equipment slot: 0..15 or a name, as for [`actions.equip`](#actionsequip). |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Errors** `no equipment slot "<name>"` for an unknown slot name; `an equipment slot is 0..15 or a name (main, sub, range, ammo, head, ...)` for another value.

**Example**

```lua
commands.register("naked", function()
  for slot = 0, 15 do
    actions.unequip(slot)
  end
end)
```

An empty slot does nothing; a slot your job's encumbrance locks is left as it is. Refused with the game's line while fishing or synthesizing.

## `actions.equip_set`

```lua
actions.equip_set(n) -> nil
```

Wears one of the game's equip sets (Edit Equip. Set, `/equipset`), and prints "Set #n equipped." in the log.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `n` | `integer` | yes | The set number, 1..200. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Errors** `equip sets are 1..200` for another number.

**Example**

```lua
-- automation: set 1 when a fight starts, set 2 when it ends.
events.on("status_change", function(status, previous)
  if status == 1 then
    actions.equip_set(1)
  elseif previous == 1 then
    actions.equip_set(2)
  end
end)
```

Refused with the game's line while fishing or synthesizing. Nothing happens in Monstrosity, for a set that has not been made, or, while the `equipset_delay` option is on (the default; `client.set_option`), within 0.9 seconds of the last set.

## `actions.lot`

```lua
actions.lot(slot) -> nil
```

Casts a lot on an item in the treasure pool, as the treasure pool window's Lot does.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `slot` | `integer` | yes | The pool slot, 0..9: the `slot` field of a `game.treasure()` entry. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- //lotall: lot on everything you have not lotted or passed yet.
commands.register("lotall", function()
  for _, t in ipairs(game.treasure()) do
    if not t.lotted and not t.passed then actions.lot(t.slot) end
  end
end)
```

A slot with no item does nothing. Refused with the game's line for a Rare item you already hold in any bag, and when your inventory has no free slot.

## `actions.pass`

```lua
actions.pass(slot) -> nil
```

Passes on an item in the treasure pool.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `slot` | `integer` | yes | The pool slot, 0..9: the `slot` field of a `game.treasure()` entry. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- //passall: pass on everything you have not lotted on.
commands.register("passall", function()
  for _, t in ipairs(game.treasure()) do
    if not t.lotted and not t.passed then actions.pass(t.slot) end
  end
end)
```

A slot with no item does nothing.

## `actions.cancel`

```lua
actions.cancel(status) -> nil
```

Cancels one of your status effects, as picking its icon in the status list does. Its icon goes at once.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `status` | `integer` | yes | The status effect id (as in `game.buffs` and `res.status`). |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- //nosneak: drop Sneak (status 71).
commands.register("nosneak", function()
  actions.cancel(71)
end)
```

Nothing happens when you do not have the effect or the game does not let it be cancelled.

## `actions.talk`

```lua
actions.talk(id) -> nil
```

Talks to an NPC, as pressing Enter on it does. Whatever it starts (a conversation, a menu, a cutscene) shows in `game.event()`; answer it with [`actions.choose`](#actionschoose) and [`actions.advance`](#actionsadvance).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The NPC's server id (`entity.id`). |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
commands.register("hello", function()
  local t = game.target()
  if not (t and t.kind == "npc") then return end
  actions.talk(t.id)
  wait_until(function() return game.event() ~= nil end, 3)
  if game.event() then print("Talking to " .. t.name) end
end)
```

Nothing happens during an event, while an earlier talk is still waiting for the server, for a player, or for an NPC that is not shown, is hidden, or is despawning. Refused with the game's line while mounted or invisible, and with "Target out of range." beyond 6 yalms. Your own Mog House door opens its exit menu.

## `actions.choose`

```lua
actions.choose(option) -> nil
```

Picks an option of the event menu showing, as moving the cursor to it and pressing Enter does.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `option` | `integer` | yes | The option's place in `game.event().menu.options`, from 1. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Errors** `options count from 1` for 0.

**Example**

```lua
-- //pick <text>: the first menu option containing the text.
commands.register("pick", function(text)
  local ev = game.event()
  if not (ev and ev.menu and text) then return end
  for i, option in ipairs(ev.menu.options) do
    if option:lower():find(text:lower(), 1, true) then
      actions.choose(i)
      return
    end
  end
end)
```

Nothing happens when no event menu is showing or the option is past the last one.

## `actions.advance`

```lua
actions.advance() -> nil
```

Moves an event's waiting text on, as pressing Enter does.

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- //skip: press on through the event's text for up to 10 seconds.
commands.register("skip", function()
  local stop = clock() + 10
  while game.event() and not game.event().menu and clock() < stop do
    actions.advance()
    sleep(0.5)
  end
end)
```

Nothing happens outside an event. With a menu showing it picks the option under the cursor, as Enter would: check `game.event().menu` first.

## `actions.trade`

```lua
actions.trade(npc, items?, gil?) -> nil
```

Hands items, gil or both to an NPC, as the trade window's Trade button does. It is sent at once, and a refusal is raised as an error.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `npc` | `integer` | yes | The NPC's server id. |
| `items` | `table` | no, default none | A list of entries, each `{slot = , count = }`. |
| `gil` | `integer` | no, default `0` | Gil to hand over with them. |

**`items` fields** (of each entry)

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `slot` | `integer` | none: required (an entry without it raises an error) | The inventory slot (an [Item](types.md#item)'s `slot` in bag 0). |
| `count` | `integer` | `1` | How many from that stack. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Errors** each prefixed `actions.trade: `: `no such NPC` (the id is not in the zone), `too far away` (more than 6 yalms), `at most 8 entries` (items plus the gil), `an empty inventory slot`, `an item that cannot be traded (worn, a linkshell item, or not that many)` (also for a count of 0), `nothing to trade` (no items and no gil); `game state is not available here (in the lobby, or outside the mod phase)` in the lobby.

**Example**

```lua
-- //give <item id> [count]: hand it to the NPC you target.
commands.register("give", function(id, n)
  local npc = game.target()
  if not npc then return end
  for _, it in ipairs(game.items(0)) do
    if it.id == tonumber(id) then
      local ok, err = pcall(actions.trade, npc.id, { { slot = it.slot, count = tonumber(n) or 1 } })
      if not ok then print(err) end
      return
    end
  end
end)
```

Whether the NPC wants the items is the server's to decide.

## `actions.buy`

```lua
actions.buy(row, count?) -> nil
```

Buys from the NPC shop that is open, as its Buy does. It is sent at once.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `row` | `integer` | yes | The `index` field of a `game.shop()` row (from 1). |
| `count` | `integer` | no, default `1` | How many to buy. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Errors** `rows count from 1` for 0; `actions.buy: no shop open` (no shop, or a guild shop); `actions.buy: no such row` (no item in that row, or a count of 0); `game state is not available here (in the lobby, or outside the mod phase)` in the lobby.

**Example**

```lua
-- //buy <item id> [count]
commands.register("buy", function(id, n)
  for _, row in ipairs(game.shop() or {}) do
    if row.item == tonumber(id) then
      actions.buy(row.index, tonumber(n) or 1)
      return
    end
  end
end)
```

Your gil and inventory space are the server's to check.

## `actions.sell`

```lua
actions.sell(slot, count?) -> nil
```

Sells from an inventory slot to the NPC shop that is open, as its Sell does. The sale and its confirmation are sent at once, without the price prompt.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `slot` | `integer` | yes | The inventory slot (an [Item](types.md#item)'s `slot` in bag 0). |
| `count` | `integer` | no, default `1` | How many to sell. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Errors** `actions.sell: no shop open` (no shop, or a guild shop); `actions.sell: not that many in the slot` (also for an empty slot or a count of 0); `game state is not available here (in the lobby, or outside the mod phase)` in the lobby.

**Example**

```lua
-- //sellall <item id>: sell every stack of it.
commands.register("sellall", function(id)
  for _, it in ipairs(game.items(0)) do
    if it.id == tonumber(id) then
      actions.sell(it.slot, it.count)
      sleep(0.5)
    end
  end
end)
```

Whether the shop takes the item is the server's to decide.

## `actions.blue_set`

```lua
actions.blue_set(slot, spell) -> nil
```

Sets a blue magic slot to a spell, as the Set Spells window does. Read the slots back with `game.job_data()` (`blue_spells`).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `slot` | `integer` | yes | The slot, 1..20. |
| `spell` | `integer` | yes | A blue magic spell id, 513..767 (0 empties the slot). |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- //blu <spell id>...: fill the slots in order.
commands.register("blu", function(...)
  for slot, id in ipairs({...}) do
    local spell = tonumber(id)
    if spell then
      actions.blue_set(slot, spell)
      sleep(1.1)
    end
  end
end)
```

Problems found when the action runs are printed in the log in red, not raised: `actions.blue_set: not a blue mage` (neither job is Blue Mage), `actions.blue_set: slot 1..20`, `actions.blue_set: not a blue magic spell`. One change goes out per second: a change within a second of the last blue magic or automaton change, or before the server has sent your setup, does nothing. Whether you know the spell and have the points for it is the server's to check.

## `actions.blue_remove`

```lua
actions.blue_remove(slot) -> nil
```

Empties a blue magic slot.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `slot` | `integer` | yes | The slot, 1..20. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
commands.register("bluclear", function()
  for slot = 1, 20 do
    actions.blue_remove(slot)
    sleep(1.1)
  end
end)
```

Problems are printed in the log in red: `actions.blue_set: not a blue mage`, `actions.blue_set: slot 1..20`, `actions.blue_set: the slot is empty`. The one-change-a-second rule of [`actions.blue_set`](#actionsblue_set) applies.

## `actions.attach`

```lua
actions.attach(slot, item) -> nil
```

Sets an automaton slot to a head, frame or attachment, as the automaton customization window does. Read the setup back with `game.job_data()` (`head`, `frame`, `attachments`).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `slot` | `integer` | yes | Counted from 0: 0 the head, 1 the frame, 2..13 the attachments. |
| `item` | `integer` | yes | The item id: a head or frame 8193..8447, an attachment 8449..8703 (0 empties the slot). |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- Your attachments' item ids.
local SET = { 8449, 8450, 8451 }
commands.register("pupset", function()
  for k, item in ipairs(SET) do
    actions.attach(k + 1, item)
    sleep(1.1)
  end
end)
```

Problems are printed in the log in red: `actions.attach: not a puppetmaster` (neither job is Puppetmaster), `actions.attach: slot 0..13`, `actions.attach: not an item for that slot`. Nothing happens when the slot already holds the item, within a second of the last blue magic or automaton change, or before the server has sent your setup.

## `actions.detach`

```lua
actions.detach(slot) -> nil
```

Empties an automaton slot.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `slot` | `integer` | yes | Counted from 0: 0 the head, 1 the frame, 2..13 the attachments. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
commands.register("pupclear", function()
  for slot = 2, 13 do
    actions.detach(slot)
    sleep(1.1)
  end
end)
```

Problems are printed in the log in red: `actions.attach: not a puppetmaster`, `actions.attach: slot 0..13`. An empty slot, or a change within a second of the last one, does nothing.

## `actions.invite`

```lua
actions.invite(name) -> nil
```

Invites a player to your party, as `/pcmd add` does.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | yes | The player's name. A target tag such as `"<t>"` works too. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- automation: invite whoever sends a tell saying "invite".
events.on("chat_message", function(text, sender, kind)
  if kind == 3 and text:lower() == "invite" then
    actions.invite(sender)
  end
end)
```

A player in the zone is invited at once. Anyone else is looked up by name with the server and invited from there, wherever they are; when the server finds nobody by that exact name (or they are offline), nothing happens and no line is shown, as when the player types `/pcmd add`. An NPC or monster is refused with the game's line. Refused with the game's line while your party is only you and your trusts, while K.O., in an event, charmed or fishing; nothing happens in Monstrosity.

## `actions.party_leave`

```lua
actions.party_leave() -> nil
```

Leaves your party, as `/pcmd leave` does.

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
commands.register("leave", function()
  actions.party_leave()
end)
```

Refused with the game's line while your party is only you and your trusts, while K.O., in an event, charmed or fishing; nothing happens in Monstrosity.

## `actions.party_kick`

```lua
actions.party_kick(name) -> nil
```

Removes a member from your party, as `/pcmd kick` does. Only the leader can; the server checks that.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | yes | The member's name, 3 to 15 letters (any case). |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
commands.register("kick", function(name)
  if name then actions.party_kick(name) end
end)
```

A name that is not 3 to 15 letters prints "Spelling of name is incorrect." and `...A command error occurred.` Refused like [`actions.party_leave`](#actionsparty_leave).

## `actions.join`

```lua
actions.join() -> nil
```

Accepts the party invitation you have, as `/join` does.

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- automation: accept invitations from friends.
local FRIENDS = { Alice = true, Bob = true }
events.on("party_invite", function(from, kind)
  if FRIENDS[from] then
    sleep(1)
    actions.join()
  end
end)
```

Refused with the game's line while your party is only you and your trusts, while K.O., in an event, charmed or fishing; nothing happens in Monstrosity.

## `actions.decline`

```lua
actions.decline() -> nil
```

Declines the party invitation you have, as `/decline` does.

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- automation: decline invitations while the mod's "busy" flag is set.
local busy = true
events.on("party_invite", function()
  if busy then actions.decline() end
end)
```

Refused like [`actions.join`](#actionsjoin).

## `actions.mount`

```lua
actions.mount(name) -> nil
```

Calls a mount by its name, as `/mount` does.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | yes | The mount's name, in any case (as `res.mount` gives it), such as `"Chocobo"`. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
commands.register("ride", function(...)
  local name = table.concat({...}, " ")
  actions.mount(name ~= "" and name or "Chocobo")
end)
```

Refused with the game's line while mounted, fishing, synthesizing, resting, sitting, in a Mog House, K.O., in an event or charmed. A name the game does not know prints `...A command error occurred.` Whether you own the mount, its recast and the place are the server's to check.

## `actions.dismount`

```lua
actions.dismount() -> nil
```

Gets off your mount, as `/dismount` does.

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- automation: get off when something attacks you.
events.on("hp_change", function(hp, previous)
  if hp < previous and game.player().status == 5 then
    actions.dismount()
  end
end)
```

Refused with the game's line while K.O., in an event, charmed or fishing; otherwise it is sent as it is.

## `actions.open_menu`

```lua
actions.open_menu(name) -> nil
```

Opens a game window as its row in the main menu does, without the main menu being open.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | yes | The row's name, exactly as below (lower case). |

| Name | Opens |
| --- | --- |
| `status` | Status |
| `equipment` | Equipment (Customization in Monstrosity) |
| `magic` | Magic |
| `items` | Items |
| `abilities` | Abilities (the monster skills in Monstrosity) |
| `party` | Party |
| `trade` | Trade |
| `search` | Search |
| `linkshell` | Linkshell |
| `synthesis` | Synthesis |
| `region` | Region Info |
| `map` | Map; in your own Mog House the moogle's menu |
| `missions` | Missions |
| `quests` | Quests |
| `key_items` | Key Items |
| `view_house` | View House |
| `bazaar` | Set Bazaar |
| `macros` | MacroPalette |
| `config` | Config |
| `help_desk` | Help Desk |
| `communication` | Communication |
| `clock` | Current Time (prints the time) |
| `shut_down` | Shut Down (asks first) |
| `log_out` | Log Out (asks first) |
| `moogle` | The nomad moogle's menu, as a nomad moogle opens it |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Errors** `no menu "<name>" (status, equipment, ...)` for another name, with the whole list.

**Example**

```lua
commands.register("bag", function()
  actions.open_menu("items")
end)
```

A row the main menu shows dimmed does nothing, as picking it there does: Party while your party is only you and trusts, Synthesis within 23 seconds of your last synthesis, the Mog House row while visiting another player's house, and Party, Synthesis and Set Bazaar in Monstrosity. Each window then makes the checks it makes when opened from the menu. The `moogle` menu opens where you stand and closes as you move away from that spot (unless the `menu_reach` option of `client.set_option` is off).

## `actions.macro`

```lua
actions.macro(row, n) -> nil
```

Runs a macro of the current set, as its Ctrl or Alt key does; a macro already running is replaced.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `row` | `string` | yes | `"ctrl"` or `"alt"` (lower case). |
| `n` | `integer` | yes | The macro, 1..10 (Ctrl+1 .. Ctrl+0). |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Errors** `a macro row is "ctrl" or "alt"`; `a macro is 1..10`.

**Example**

```lua
-- automation: run Alt+1 of the current set when a fight starts.
events.on("status_change", function(status)
  if status == 1 then actions.macro("alt", 1) end
end)
```

Its lines run as macro lines, with the game's rules for them (`/wait`, `<wait>`, the target cursor). Nothing happens before the macros have loaded.

## `actions.macro_set`

```lua
actions.macro_set(book, set) -> nil
```

Switches the macro palette to a book and set, as the palette's keys do. The set you leave is saved first.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `book` | `integer` | yes | The book, 1..40. |
| `set` | `integer` | yes | The set in that book, 1..10. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- automation: book 1 for warrior (job 1), book 2 for anything else; set 1.
events.on("job_change", function(main)
  actions.macro_set(main == 1 and 1 or 2, 1)
end)
```

Numbers out of range are moved into it (0 becomes 1, 50 becomes 40). Nothing happens before the macros have loaded. Read the palette back with `game.macros()`.

## `actions.macro_edit`

```lua
actions.macro_edit(row, n, line, text) -> nil
```

Changes one line or the title of a macro in the current set, and saves the set to the macro files, as the Edit Macro window does.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `row` | `string` | yes | `"ctrl"` or `"alt"`. |
| `n` | `integer` | yes | The macro, 1..10. |
| `line` | `integer\|string` | yes | The line, 1..6, or `"title"` for its title. |
| `text` | `string` | yes | The new text: at most 60 bytes for a line, 8 for a title (Shift-JIS: a plain letter is one byte). An empty string clears it. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Errors** `a macro line is 1..6, or "title"`; `a macro row is "ctrl" or "alt"`; `a macro is 1..10`.

**Example**

```lua
-- //mkcure: Ctrl+1 casts Cure on the target cursor.
commands.register("mkcure", function()
  actions.macro_edit("ctrl", 1, "title", "Cure")
  actions.macro_edit("ctrl", 1, 1, '/ma "Cure" <st>')
  actions.macro_edit("ctrl", 1, 2, "")
end)
```

Text longer than the limit does nothing (no error). Nothing happens before the macros have loaded.

## `actions.scan`

```lua
actions.scan() -> nil
```

Asks the server for the Wide Scan list, as the map's Wide Scan does; the results show in the game's Wide Scan list on the map.

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
commands.register("rescan", function()
  actions.scan()
end)
```

A new scan clears the last results and any tracking. While the server is still sending the last list, the call ends tracking instead. Whether you can scan is the server's to decide.

## `actions.track`

```lua
actions.track(index) -> nil
```

Tracks an entity with Wide Scan, as picking it in the Wide Scan list does. Once the server confirms, `"<scan>"` (in target arguments and `game.entity_by_tag`) names it.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `index` | `integer` | yes | The entity's index in the zone (`entity.index`), not its server id. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- //hunt <name>: track the first monster in range with that name.
commands.register("hunt", function(...)
  local want = table.concat({...}, " ")
  for _, e in ipairs(game.entities({ kinds = { "mob" } })) do
    if e.name == want then
      actions.track(e.index)
      return
    end
  end
end)
```

The server answers nothing when it refuses (out of its range).

## `actions.untrack`

```lua
actions.untrack() -> nil
```

Stops Wide Scan tracking.

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
commands.register("untrack", function()
  actions.untrack()
end)
```

## `actions.run`

```lua
actions.run(heading?) -> nil
```

Starts running on by itself (autorun) in a direction, as the autorun key does.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `heading` | `number` | no, default the way you face | The direction in radians, in the units of `game.player().heading`: 0 east, π/2 south, π west, 3π/2 north. Any angle works (negative too). |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- //dash: run north for three seconds.
commands.register("dash", function()
  actions.run(3 * math.pi / 2)
  sleep(3)
  actions.stop()
end)
```

It ends a follow or a run to a point that was going on. The player's turn keys still steer it and the autorun key stops it. It stops by itself in an event, when K.O., resting, sitting, fishing and the like, and on a zone change; while you are locked on to a target it does not move you.

## `actions.run_to`

```lua
actions.run_to(x, z, within?) -> nil
```

Runs straight toward a point on the ground and stops when you are close enough. The direction is set again every frame, so it reaches the point even when something pushes you aside, but there is no path finding: walls stop you.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `x` | `number` | yes | The point's x (as in `pos.x`). |
| `z` | `number` | yes | The point's z (as in `pos.z`). |
| `within` | `number` | no, default `1` | Stop this many yalms from the point (at least 0.1). |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
local function near(x, z, d)
  local p = game.player().pos
  return (p.x - x) ^ 2 + (p.z - z) ^ 2 <= d * d
end
-- //goto: run up to where the target stands.
commands.register("goto", function()
  local t = game.target()
  if not t then return end
  actions.run_to(t.pos.x, t.pos.z, 3)
  wait_until(function() return near(t.pos.x, t.pos.z, 3) end, 30)
  if near(t.pos.x, t.pos.z, 3) then print("Arrived.") end
end)
```

Nothing happens when you are already within the distance. It ends like [`actions.run`](#actionsrun), and [`actions.stop`](#actionsstop) ends it too. To know when you have arrived, compare `game.player().pos` with the point in a `wait_until` test.

## `actions.follow`

```lua
actions.follow(id) -> nil
```

Follows an entity, running after it, as `/follow` does.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | The entity's server id. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- //fol <name>: follow a party member.
commands.register("fol", function(name)
  for _, m in ipairs(game.party()) do
    if m.name == name and m.in_zone then
      actions.follow(m.id)
      return
    end
  end
end)
```

An id that is not in the zone raises an error, and an entity with no model, or dead, gets the game's refusal line. The follow ends, with "Follow canceled." in the log, when the run stops for any reason: [`actions.stop`](#actionsstop), the autorun key, the entity going away or dying, and the reasons of [`actions.run`](#actionsrun). The command line's checks of `actions.cast` apply, but it is allowed while mounted.

## `actions.walk`

```lua
actions.walk(on) -> nil
```

Switches between walking (`true`) and running (`false`), as the walk key does. It is the same switch the player's movement uses.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `on` | `boolean` | yes | `true` to walk, `false` to run. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
local walking = false
commands.register("stroll", function()
  walking = not walking
  actions.walk(walking)
end)
```

## `actions.stop`

```lua
actions.stop() -> nil
```

Stops a run started by [`actions.run`](#actionsrun), [`actions.run_to`](#actionsrun_to) or [`actions.follow`](#actionsfollow) (or by the autorun key).

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- automation: stop running when a fight starts.
events.on("status_change", function(status)
  if status == 1 then actions.stop() end
end)
```

Stopping a follow prints "Follow canceled." in the log.

## `actions.turn`

```lua
actions.turn(heading) -> nil
```

Turns your character to face a direction.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `heading` | `number` | yes | The direction in radians, in the units of `game.player().heading`: 0 east, π/2 south, π west, 3π/2 north. Any angle works. |

**Returns** nothing.

**Needs** the `actions` permission (while handling the player's input) or `automation`.

**Example**

```lua
-- //face: face the target.
commands.register("face", function()
  local me, t = game.player(), game.target()
  if t then
    actions.turn(math.atan(me.pos.z - t.pos.z, t.pos.x - me.pos.x))
  end
end)
```

The direction is kept to 256 steps a circle. You keep facing it while you stand still, until you move, lock on, or the camera turns you.
