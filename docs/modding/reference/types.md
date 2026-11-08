# Types

The tables the API hands to mods, or takes from them, that more than one function shares. Every table the API returns is a fresh copy made for that call: reading it is cheap, and changing it changes nothing in the game. Positions are world coordinates in yalms (x and z on the ground, y the height with y pointing down, so a smaller y is higher), headings are radians from 0 to 2π (0 east, π/2 south), and an entity's `id` is its server id while `index` is its index in the zone.

## Player

Your own character, as `game.player()` returns it. Fields that need data from the server are nil until it has arrived.

| Field | Type | Description |
| --- | --- | --- |
| `name` | `string` | Your character's name. |
| `id` | `integer` | Your server id. |
| `index` | `integer` | Your index in the zone. |
| `zone` | `integer?` | The zone id, as `game.zone()` gives it. |
| `pos` | `{x: number, y: number, z: number}` | Where you stand. |
| `heading` | `number` | The way you face, in radians (0 east, π/2 south). |
| `status` | `integer` | The server's status: 0 idle, 1 engaged, 2 and 3 K.O., 5 riding a chocobo, 33 resting; other values for other states. |
| `hp` | `integer?` | Current HP. |
| `mp` | `integer?` | Current MP. |
| `tp` | `integer?` | Current TP. |
| `hpp` | `integer?` | HP percent. |
| `mpp` | `integer?` | MP percent. |
| `hp_max` | `integer?` | Maximum HP. |
| `mp_max` | `integer?` | Maximum MP. |
| `main_job` | `integer?` | Main job id (`res.job(id)` gives its abbreviation). |
| `main_level` | `integer?` | Main job level. |
| `sub_job` | `integer?` | Support job id. |
| `sub_level` | `integer?` | Support job level. |
| `target` | `integer?` | Your target's server id; nil without a target. |
| `focus_target` | `integer?` | The focus target's server id (`/focustarget`: a member of your party, not you); nil without one. |
| `last_target` | `integer?` | The target before the current one, while it is here and can be targeted; nil otherwise. |
| `away` | `boolean` | The /away flag as the server last sent it (/online clears it). |
| `anonymous` | `boolean` | The /anon flag. |
| `seeking` | `boolean` | Seeking a party (the search comment's flag). |

`hp`, `mp`, `tp`, `hpp` and `mpp` are nil until the server has sent your vitals; `hp_max`, `mp_max` and the four job fields until it has sent your stats.

```lua
local p = game.player()
if p.hpp and p.hpp < 50 then
  print(("%s is at %d%% HP (%d / %d)"):format(p.name, p.hpp, p.hp, p.hp_max or 0))
end
```

## Entity

A character, NPC or monster in the zone, from `game.entity`, `game.entities`, `game.target` and `game.entity_by_tag`. Your own character is an Entity too (with `me = true`); a few fields differ for it, as noted.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `integer` | Its server id. |
| `index` | `integer` | Its index in the zone. |
| `name` | `string` | Its name. |
| `kind` | `string` | `"player"`, `"npc"` or `"mob"` (a monster). |
| `pos` | `{x: number, y: number, z: number}` | Where it is. |
| `heading` | `number` | The way it faces, in radians (0 east, π/2 south). |
| `distance` | `number` | Its straight-line distance from you in yalms, height included; 0 for yourself. |
| `hpp` | `integer` | HP percent. Yours is 100 until the server has sent your vitals. |
| `gauge` | `boolean` | It has an HP gauge: the target window shows one for it. |
| `status` | `integer` | The server's status, as `Player.status`. |
| `speed` | `number` | Its movement speed, in yalms per second. |
| `claim` | `integer` | The server id of whoever has claimed it; 0 when nobody has. Always 0 for yourself. |
| `targetable` | `boolean` | It can be targeted. Always true for yourself. |
| `hidden` | `boolean` | It is hidden outside events. Always false for yourself. |
| `master` | `integer` | For a pet or trust, its master's index; 0 for none. |
| `me` | `boolean?` | true for your own character; nil for everyone else. |
| `race` | `integer?` | Its race number; nil when unknown. |
| `model` | `integer?` | The model id of an NPC or monster; 0 for one drawn from a look. nil for your own entity. |
| `look` | `table?` | A character-style look: the model ids `{face, head, body, hands, legs, feet, main, sub, range}` (all integers). nil when it has none; your own is the look your model was built from (before any `render.look`). With `race` added, it is what `res.model_files` takes. |
| `in_party` | `boolean` | It is in your party. true for yourself. |
| `in_alliance` | `boolean` | It is in your party or alliance. true for yourself. |
| `hitbox` | `number` | Its model radius as the server gives it, in yalms. |
| `height` | `number?` | Its model's height in yalms, standing at rest (a hat counts); nil until its model is loaded. Its head is near `pos.y - height` (y goes down). |
| `bones` | `integer` | Its model's skeleton bones: a few for an object (a treasure casket has 3), dozens for a creature or a character; 0 with no skinned model (a ??? point, a door) or until its model is loaded. |
| `target_index` | `integer?` | The index of what it is targeting (for yourself: your target's index); nil when none. |
| `target_id` | `integer?` | That target's server id, when it is known; nil otherwise. |
| `pet` | `boolean?` | true for your own pet; nil for anything else. |
| `mpp` | `integer?` | Your pet only: its MP percent. |
| `tp` | `integer?` | Your pet only: its TP. |
| `pet_target` | `integer?` | Your pet only: the server id of what it fights, when it has a target. |

```lua
local t = game.target()
if t and not t.me then
  local who = t.claim ~= 0 and game.entity(t.claim)
  print(t.name, t.kind, ("%.1f yalms"):format(t.distance), who and ("claimed by " .. who.name) or "unclaimed")
end
```

## Member

A party or alliance member, from `game.party()`. Members in another zone are listed too.

| Field | Type | Description |
| --- | --- | --- |
| `slot` | `integer` | The party window slot, 0 to 17: 0 to 5 your party (0 is you), 6 to 11 and 12 to 17 the alliance's other parties. Matches the target tags `<p0>`..`<p5>`, `<a10>`..`<a15>`, `<a20>`..`<a25>`. |
| `party` | `integer` | Which party: 0 yours, 1 and 2 the alliance's others. |
| `id` | `integer` | The member's server id. |
| `index` | `integer` | The member's index in the zone. |
| `name` | `string` | The member's name. |
| `hp` | `integer` | Current HP. |
| `mp` | `integer` | Current MP. |
| `tp` | `integer` | Current TP. |
| `hpp` | `integer` | HP percent. |
| `mpp` | `integer` | MP percent. |
| `zone` | `integer` | The member's zone id, as the server last gave it. |
| `in_zone` | `boolean` | The member is in your zone. |
| `main_job` | `integer` | Main job id. |
| `main_level` | `integer` | Main job level. |
| `sub_job` | `integer` | Support job id. |
| `sub_level` | `integer` | Support job level. |
| `leader` | `boolean` | The member leads its party. |
| `alliance_leader` | `boolean` | The member leads the alliance. |
| `buffs` | `integer[]` | The status effect ids the server sent for the member (use `game.buffs()` for your own). |

```lua
for _, m in ipairs(game.party()) do
  if m.party == 0 and m.in_zone and m.hpp < 75 then
    print(m.name .. (m.leader and " (leader)" or ""), m.hpp .. "%", res.job(m.main_job))
  end
end
```

## Item

An item in a bag slot, from `game.items(bag)`.

| Field | Type | Description |
| --- | --- | --- |
| `bag` | `integer` | The bag's number (0 inventory, 1 Mog Safe, ...; see `game.items`). |
| `slot` | `integer` | The slot in that bag, from 1. |
| `id` | `integer` | The item id (`res.item(id)` gives its record). |
| `count` | `integer` | How many are in the slot. |
| `extdata` | `string?` | The item's 24 bytes of extra data (augments and the like; `res.augments(id, extdata)` decodes them). nil when the server has sent none. |
| `locked` | `integer` | The slot's lock flag: 0 when the item is free, another value while it is locked (25 while it is for sale in your bazaar). |
| `price` | `integer?` | A price the server sent with the slot: your bazaar price when `locked` is 25. nil when none was sent. |

```lua
for _, it in ipairs(game.items(0)) do
  if it.locked == 25 then
    local r = res.item(it.id)
    print(r and r.name or it.id, "on sale for", it.price)
  end
end
```

## Action

An action decoded from the server (an attack round, a spell, an ability, a weapon skill...): the argument of the `action` event. One action has an actor, one or more targets, and one or more results per target.

| Field | Type | Description |
| --- | --- | --- |
| `actor` | `integer` | The server id of who acts. |
| `category` | `integer` | The kind of action, as the server numbers it. |
| `param` | `integer` | The spell or ability id; for some categories a four-character code packed into the integer. |
| `info` | `integer` | Extra information (for a finished spell, its recast). |
| `targets` | `table[]` | The targets, each `{id, results}`: `id` the target's server id, `results` a list of results. |

Each result:

| Field | Type | Description |
| --- | --- | --- |
| `miss` | `integer` | 0 hit, 1 miss, 2 guard, 3 parry, 4 block. |
| `kind` | `integer` | The result's kind. |
| `animation` | `integer` | The animation id. |
| `info` | `integer` | Flags: bit 0 defeated, bit 1 critical. |
| `scale` | `integer` | The hit's distortion and knockback. |
| `value` | `integer` | The result's number (damage, HP healed...). |
| `message` | `integer` | The battle message id (`res.message(id, ...)` expands it). |
| `bit` | `integer` | Further result bits. |
| `proc` | `table?` | An additional effect, `{kind, info, value, message}`; nil when none. |
| `react` | `table?` | A reaction (such as spikes or a counter), `{kind, info, value, message}`; nil when none. |

```lua
events.on("action", function(a)
  if a.actor ~= game.player().id then return end
  for _, t in ipairs(a.targets) do
    for _, r in ipairs(t.results) do
      print("category", a.category, "->", t.id, "message", r.message, "value", r.value)
    end
  end
end)
```

## WindowSpec

The definition of a mod window, the second argument of `ui.window(id, spec)`. This table lists the window-level fields; the elements (text, buttons, lists, sliders...) are documented in [ui.md](ui.md).

| Field | Type | Description |
| --- | --- | --- |
| `title` | `string?` | The window's title. Default `""`. |
| `width` | `number?` | The window's width: points in the modern UI, the menu layout's pixels in the classic UI. None or 0: the default width (366 in the classic UI). |
| `help` | `string?` | The help line shown while the element under the cursor has no `tip` of its own. |
| `hud` | `boolean?` | true: the window never takes the keys (a click on it still acts). Default false. |
| `frame` | `boolean?` | false: drawn without a frame. Default true. |
| `x` | `number?` | The window's left edge in the classic UI. Used only together with `y`. |
| `y` | `number?` | The window's top edge in the classic UI. Used only together with `x`. |
| `modal` | `boolean?` | true: the window keeps the keys and the pointer while it is open; centred unless placed with `x` and `y`. Default false. |
| `popup` | `boolean?` | true: a modal window opened at the pointer and closed by a click outside it. Default false. |
| `opacity` | `number?` | The whole window's opacity, 0..1 (per-window opacity). Default 1. |
| `elements` | `table[]?` | The window's elements in order, each with a `type` field (see [ui.md](ui.md)). |

```lua
ui.window("hello", {
  title = "Hello",
  width = 240,
  help = "A small window.",
  elements = { { type = "text", id = "line", text = "Hi there" } },
})
ui.open("hello")
```

## Packet

A packet as a hook from `packets.on_in` or `packets.on_out` sees it. The hook returns nil to leave the packet alone, false to block it, or a string of new bytes to replace it (changing or blocking needs the `packets.write` permission).

| Field | Type | Description |
| --- | --- | --- |
| `id` | `integer` | The packet id, from its header (0 to 511). |
| `data` | `string` | The whole sub-packet as it stands now, header included: with the changes of hooks that ran before this one. |
| `original` | `string` | The sub-packet as it arrived or was built, before any hook changed it. |
| `injected` | `boolean` | A mod injected it (`packets.inject_in` / `inject_out`). |
| `blocked` | `boolean` | A hook before this one blocked it. |
| `outgoing` | `boolean` | true for a packet being sent (an `on_out` hook), false for one received. |

A replacement shorter than 4 bytes is ignored; a longer one is padded to a whole number of 4-byte words and its header's size is set to fit.

```lua
packets.on_in(0x00D, function(p)
  if p.injected then return end
  print(("packet 0x%03X: %d bytes%s"):format(p.id, #p.data, p.blocked and " (blocked)" or ""))
end)
```

## KeyEvent

A key going down, repeating or coming up, as an `input.on_key` filter sees it.

| Field | Type | Description |
| --- | --- | --- |
| `key` | `string` | The key's name, lower case: `a`..`z`, `0`..`9`, `f1`..`f12`, `escape`, `enter`, `space`, `tab`, `backspace`, `insert`, `delete`, `home`, `end`, `pageup`, `pagedown`, `up`, `down`, `left`, `right`, `-`, `=`, `[`, `]`, `\`, `;`, `'`, `` ` ``, `,`, `.`, `/`, `numpad0`..`numpad9`, `numpad+`, `numpad-`, `numpad*`, `numpad/`, `numpad.`, `numpadenter`, `capslock`, `scrolllock`, `numlock`, `pause`, `printscreen`, `apps`, `lshift`, `rshift`, `lctrl`, `rctrl`, `lalt`, `ralt`, `lwin`, `rwin`. Other keys never reach the filters. |
| `down` | `boolean` | true for a press or a repeat, false for the release. |
| `repeated` | `boolean` | true for a repeat while the key is held. |
| `ctrl` | `boolean` | A Ctrl key is held. |
| `alt` | `boolean` | An Alt key is held. |
| `shift` | `boolean` | A Shift key is held. |
| `typing` | `boolean` | A text field (the chat line, a search...) has the keys. |
| `blocked` | `boolean` | An earlier mod's filter took this key. |

Once a filter takes a press (returns true, with the `input.capture` permission), the key's repeats are taken without reaching the filters, and its release is taken as well.

```lua
input.on_key(function(e)
  if e.key == "f9" and e.down and not e.repeated and e.ctrl and not e.typing then
    print("Ctrl+F9")
  end
end)
```

## MouseEvent

The mouse as an `input.on_mouse` filter sees it.

| Field | Type | Description |
| --- | --- | --- |
| `kind` | `string` | `"down"` (a button pressed), `"up"` (released), `"move"` or `"wheel"`. |
| `button` | `string?` | For `down` and `up`: `"left"`, `"right"`, `"middle"`, `"back"`, `"forward"` or `"other"`. nil for `move` and `wheel`. |
| `x` | `integer` | The pointer's x in window pixels. |
| `y` | `integer` | The pointer's y in window pixels. |
| `delta` | `number?` | For `wheel`: how far it turned, in notches, up positive (a touchpad's scroll counts 40 pixels a notch). nil otherwise. |
| `blocked` | `boolean` | An earlier mod's filter took this event. |

A left or right press on a HUD element that has a click handler (`hud.on`) goes to that handler; the filters see it with `blocked = true`.

```lua
input.on_mouse(function(e)
  if e.kind == "wheel" and e.delta > 0 then
    print("Wheel up at", e.x, e.y)
  end
end)
```

## Time

The Vana'diel date and clock, from `game.time()`. Before the server has sent the time, every field is worked out from an `earth` of 0.

| Field | Type | Description |
| --- | --- | --- |
| `earth` | `integer` | The game clock: Earth seconds since the Vana'diel epoch. |
| `year` | `integer` | The Vana'diel year. |
| `month` | `integer` | The month, 1 to 12. |
| `day` | `integer` | The day of the month, 1 to 30. |
| `hour` | `integer` | The hour, 0 to 23. |
| `minute` | `integer` | The minute, 0 to 59. |
| `weekday` | `integer` | The day of the week, 0 to 7, 0 Firesday (`res.day(weekday)` gives its name). |
| `moon_phase` | `integer` | The moon's phase, 0 to 11 (`res.moon(phase)` gives its name). |
| `moon` | `integer` | The moon's percent, 0 to 100. |

```lua
local t = game.time()
if t.hour >= 20 or t.hour < 4 then
  print(("Night: %02d:%02d on %s, moon %d%%"):format(t.hour, t.minute, res.day(t.weekday), t.moon))
end
```

## Mod

The running mod itself: the global table `mod`, set before the mod's entry file runs.

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | The mod's id. |
| `name` | `string` | The mod's name, from its mod.toml. |
| `version` | `string` | The mod's version, from its mod.toml. |
| `api` | `integer` | The version of the mod API this client provides (now 1). |
| `path` | `string` | The mod's folder; for a mod bundled with the client, `bundled/<id>`. |

```lua
print(("%s %s (API %d) loaded from %s"):format(mod.name, mod.version, mod.api, mod.path))
```
