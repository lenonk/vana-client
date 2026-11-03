# Lua mods: add-on coverage

The bar for the mod API (docs/lua-mods.md, section 1): every existing Windower and Ashita add-on and plugin must be
re-implementable on it. Running them unmodified does not matter. This table tracks that, one row per add-on, plugin or
feature, rated against the API as of 2 November 2026: the generated stubs
(`crates/vana-client/assets/mods/library/vana.lua`) and the State list in docs/lua-mods.md. A row that is not covered
points at a gap in the API; when a gap is filled, re-rate the rows it lists.

An add-on found in more than one set (a Windower and an Ashita version, a v3 and a v4 one) has a row in each. The gap
lists name the set after a name that repeats.

## Status and conventions

- `covered`: a mod can re-implement it with the calls named, or the client already does the job natively, or it fixes a
  problem our client does not have ("not needed").
- `partly`: the core works; the gaps outside parentheses are missing.
- `missing`: the core needs something the API does not offer.
- A gap in parentheses does not block its row: it would shorten the port, nothing more.

Settled for every row:
- Raw packet bytes with `string.unpack` / `string.pack` are enough. Packet field layouts and decoded views are a
  non-blocking `(packet layouts)` gap. The same holds for item extdata other than augments: `Item.extdata` has the 24
  raw bytes, and `res.augments` decodes augments.
- Packet batch order (equips ahead of an action) does not block: a hook blocks the action packet and sends the equips
  and then the action with `packets.inject_out`, which keeps their order.
- HUD fonts do not block: `hud.text` has the game font, the UI's regular and bold faces, outlines and shadows. Other
  font families are a non-blocking `(font families)` gap.
- Native client features count as covered: draw distance, frame limit, minimap, timestamps, the Timers Extra, party
  status icons, screenshots and the like.
- Fixes for problems our client does not have count as covered ("not needed").
- Gamepad input is deferred until after the first release. It stays a gap, marked "(deferred)": it blocks the controller
  add-ons and is in parentheses where a pad is one input among others.
- Windower's Timers service is `timers.add`, rows in the client's Timers window. Any other service one add-on offers
  others counts as covered when that add-on can be ported: `mods.provide` and `commands.register` make the service.
- Data the packets carry counts as state: quest logs (0x055 / 0x056), invites (0x0DC). Derived events for them would be
  conveniences.
- Data a server leaves out is not an API gap. `Entity.target_index` is the face target the server sends in 0x00D /
  0x00E; where a monster's battle target is not there, the `action` event shows whom it acts on. Party members' casts
  come the same way (category 8 starts); `game.cast` is our cast bar.
- Every player look comes from 0x00A / 0x00D / 0x051, so look and anti-blink changes by packet rewrite are covered; look
  slots in `render.entity` would be a convenience.
- A protocol a mod can frame itself over `net.connect` (WebSocket) does not block; nor do arcs, which a canvas draws as
  runs of lines.
- Short name tables a mod can carry (weather, elements, mounts) do not block.

## Summary

| Set | Rows | Covered | Partly | Missing |
| --- | --: | --: | --: | --: |
| Windower official add-ons | 128 | 128 | 0 | 0 |
| Windower plugins | 17 | 17 | 0 | 0 |
| Windower deprecated plugins | 26 | 26 | 0 | 0 |
| Windower features | 11 | 11 | 0 | 0 |
| Windower community add-ons | 55 | 54 | 1 | 0 |
| Windower community capabilities | 13 | 12 | 0 | 1 |
| Ashita v4 official add-ons | 81 | 79 | 0 | 2 |
| Ashita v3 official add-ons | 13 | 13 | 0 | 0 |
| Ashita v4 plugins | 10 | 10 | 0 | 0 |
| Ashita v3 plugins | 27 | 27 | 0 | 0 |
| Ashita community plugins | 37 | 36 | 1 | 0 |
| Ashita v4 community add-ons | 147 | 144 | 1 | 2 |
| Ashita v3 community add-ons | 22 | 22 | 0 | 0 |
| Ashita add-ons rated from descriptions | 28 | 28 | 0 | 0 |
| Windower, all | 250 | 248 | 1 | 1 |
| Ashita, all | 365 | 359 | 2 | 4 |
| **Total** | **615** | **607** | **3** | **5** |

## Open gaps

Ranked by the rows each one blocks (listed outside parentheses).

1. **gamepad input (deferred)**, 6 rows: pad buttons and sticks as events and binds (deferred with gamepad support).
   Blocks Capability: gamepad input, cBind, Crossbar, tCrossBar, trigger, XIVCrossbar.
2. **native list options**, 1 row: options for native lists (the spell list trimmed; what its shortcut list is was not
   found). Blocks trimspells.
3. **native state fix**, 1 row: a fix for a client state bug on fast job swaps (not checked whether ours has it). Blocks
   fastswap.

Most wanted non-blocking gaps (rows that list them in parentheses):

- **packet layouts**, 102 rows: named fields and decoded views for packet ids (raw bytes work today).
- **font families**, 51 rows: named system fonts for `hud.text` (it has the game font, the UI's regular and bold faces,
  outlines and shadows).
- **packet batch order**, 9 rows: inserting packets ahead of one in a hook (block and re-inject works today). In
  Ashitacast, Ashitacast / LegacyAC, dlac, GearSwap, GearSwap framework libs, GearSwap job-file API, LuAshitacast,
  Mote-based GearSwap job files, Spellcast.
- **extdata layouts**, 8 rows: decoded extdata other than augments: slips, charges, use timers, points. In Bonanza,
  Dimmer, findAll (Windower), latentchecker, MyHome, organizer, porter (Windower), PorterPacker.
- **gamepad input (deferred)**, 6 rows: pad input. In Binder, ffxi-balloon-ashitav4, GlamourUI, Warn, XIUI, yunaui.
- **look / race override in render**, 6 rows: look, race and face slots in `render.entity`. In BlinkMeNot, Cosplay,
  DressUp, GearLock, PlasticSurgeon, Stylist.
- **dialogue text by id**, 3 rows: zone dialogue text by message id. In boxdestroyer (Windower), omen (Ashita v4
  community), pointwatch.
- **engine settings from Lua**, 3 rows: frame limit, draw distance from Lua (native settings cover them). In autofps,
  drawdistance (Ashita v4), fps.
- **inline colours in window text**, 3 rows: several colours inside one text or list row (a text element has one
  colour). In chatty, lschat, xichats.
- **native window bounds**, 3 rows: where the game's windows are drawn. In PartyBuffs (Ashita v3 community), partybuffs
  (Ashita v4 community), TParty (Windower).
- **camera rotation and pitch limits**, 2 rows: limits on how far the camera turns and tilts (`render.set_camera` has
  distance, range, speed and battle distance). In xicamera (Ashita v3 community), xicamera (Ashita v4 community).
- **effect bubble override in render**, 2 rows: hiding or changing effect bubbles in `render`. In indinope,
  VisibleFavor.
- **quest / mission state in game**, 2 rows: quest and mission logs in `game` (0x055 / 0x056 raw today). In questmarks,
  XIchecklist.
- **res merits**, 2 rows: merit names in `res`. In GearSwap, GearSwap job-file API.
- **res monstrosity**, 2 rows: Monstrosity species and instincts in `res`. In GearSwap, shortcuts.
- **SQLite**, 2 rows: SQLite (JSON files do the job). In captain, Trust.
- One row each: `<lastst>` target tag (SubTarget); arc primitive (nextauto); encumbrance flags (equipviewer (Windower));
  job change action (JobChange); linkshell name / linkshell_change (chatPorter); multi-line text input (repl); res item
  grammar (battlemod); res log-form status names (SimpleLog); res spell effects and durations (Debuffed); res.ability
  range (DistancePlus); RoE objectives in game (roe); stopping a started program (Run); synthesis event (craft);
  treasure drop time (TreasurePool); WebSocket framing (buddypal); Wide Scan results (widescan).

## The tables

### Windower official add-ons

| Add-on | What it does | API used | Status | Missing |
| --- | --- | --- | --- | --- |
| AnnounceTarget | Tells the party about listed mobs when targeted | `target_change`; game.target / entity; commands.run('/p ...'); settings; commands.register | covered | - |
| answeringmachine | Records tells for later recall | `chat_message` (kind 3); filters.typed (sent tells); input.on_key / on_mouse (idle); hud.text; storage | covered | - |
| autocontrol | PUP attachment sets, burden tracker | actions.attach / detach; game.job_data; `action`; `buff_gain` / `buff_lose`; game.recasts; res.item / ability; hud.text; timers.add | covered | - |
| autoenterkey | Presses Enter at start to clear the login warning, then unloads | mod.toml `lobby`; `lobby_screen`; input.press | covered | - |
| autoex | Runs commands on game events, rules in XML | events.on (vitals, buffs, time, day, moon, weather, zone, job, level, chat); `battle_message` (exp); game.equipment (no ammo); commands.run; load; fs; `emote` / `party_invite` / `examined`; res.weather / weather_element / element | covered | - |
| autoinvite | Invites players who send a keyword tell | `chat_message`; actions.invite; game.party | covered | - |
| autojoin | Accepts or declines party invites by list | packets.on_in(0x0DC); actions.join / decline; settings; `emote` / `party_invite` / `examined` | covered | (packet layouts: 0x0DC) |
| AutoRA | Repeats ranged attacks like auto-attack | `action`; actions.ranged; binds.set; settings | covered | - |
| azureSets | Saved BLU spell sets | actions.blue_set / blue_remove; game.job_data; game.spells; res.spell; `job_change`; timers.add | covered | - |
| barfiller | Experience / limit bar above the help window | game.stats; `battle_message`; `level_up` / `level_down`; hud.bar (kind experience); hud.text; hud.screen; hud.image | covered | - |
| battlemod | Condensed, coloured battle text | `action`; `battle_message`; filters.chat (battle lines dropped); chat.add; game.party / entity; res.item / spell / ability / status; res.message; res.mob_skill | covered | (res item grammar) |
| BattleStations | Changes or removes battle music | packets.on_in / inject_in(0x05F); packets.last('in', 0x00A); `status_change`; game.party; audio.music / mute_music | covered | (packet layouts: 0x00A / 0x05F) |
| blist | Blocks chat from listed players per mode, synced by IPC | filters.chat; ipc.send / `ipc_message`; commands.register | covered | - |
| BLUAlert | Line and sound when a mob readies an unlearned blue spell | `action` (category 7); game.spells; res.spell; chat.add; audio.play; res.mob_skill | covered | - |
| bluguide | Clickable BLU spell menu | ui.window or hud.text + hud.on; input.on_mouse; actions.blue_set / blue_remove; game.job_data / spells | covered | - |
| Bonanza | Buys and judges Bonanza marbles | actions.talk; packets.on_in (block 0x034) / inject_out(0x05B); filters.chat; game.items (extdata) | covered | (packet layouts: 0x034 / 0x05B; extdata layouts) |
| boxdestroyer | Treasure casket combination solver | packets.on_in / on_out (raw); filters.chat (hint lines); game.event; game.entity / items | covered | (dialogue text by id; packet layouts: 0x02A / 0x034 / 0x05B) |
| cancel | Cancels buffs by name or id | actions.cancel; game.buffs; res.status; commands.register | covered | - |
| capetrader | Automates Ambuscade cape augments | actions.trade; actions.talk; game.event / actions.choose or packets (0x034 block, 0x05B inject); game.items (extdata); res.augments | covered | (packet layouts: 0x032 / 0x034 / 0x05B) |
| cBlock | Blocks FFOChat lines (service offline) | filters.chat; fs; commands.register | covered | - |
| cellhelp | Salvage cells, lot order, lot profiles | filters.chat; hud.text; game.player; game.treasure; actions.lot / pass; timers.add | covered | (font families) |
| chars | `<tags>` to special characters in typed text | filters.typed; text.to_shift_jis | covered | - |
| ChatLink | Numbered links for URLs in chat | filters.chat; net.open_url; client.copy | covered | - |
| chatmon | Sound alerts on chat, tells, emotes, examines, invites | `chat_message`; filters.chat; audio.play; text.plain; client.focus; `focus_change`; `emote` / `party_invite` / `examined` | covered | - |
| chatPorter | Relays tell / party / linkshell chat between game windows | `chat_message`; ipc.send / `ipc_message`; hud.text; commands.run('/t ...') | covered | (linkshell name / linkshell_change; font families) |
| checkparam | Sums the gear stats of a /checked player | packets.on_in(0x0C9); res.item (description); game.entity; res.augments | covered | (packet layouts: 0x0C9) |
| Clock | On-screen clock in several time zones | hud.text; os.date; timer.every | covered | - |
| ConsoleBG | Background behind the Windower console | hud.rect; chat.line().open; not needed: no separate console | covered | - |
| craft | Synthesis automation, support NPCs, crystals | commands.run('/item ...', '/lastsynth'); packets.inject_out(0x096); actions.move / sort; actions.talk / choose; input.press; game.items | covered | (synthesis event; packet layouts: 0x096 / 0x06F / 0x034) |
| Debuffed | Debuffs on the target | `action`; `battle_message`; `target_change`; res.spell; hud.text | covered | (res spell effects and durations) |
| diagnostics | Checks the Windower / POL install, report to clipboard | game.info; mods.list; client.copy; not needed: no such install to check | covered | - |
| digger | Chocobo digging stats and dig delay | filters.chat; packets.on_in(0x02A / 0x02F); game.items; timers.add | covered | (packet layouts: 0x02A / 0x02F) |
| Dimmer | Warps to Reisenjima with a Dim ring, on every window | actions.equip / use_item; game.items (extdata); ipc.send | covered | (extdata layouts: charges, use timer) |
| distance | Distance to the target | game.target().distance; hud.text; `frame` | covered | - |
| DistancePlus | Distance and height coloured by job ranges, pet distance | game.target; game.entity_by_tag('pet'); game.abilities; res.spell (range); hud.text | covered | (res.ability range) |
| DressUp | Changes shown gear models, race and face | packets.on_in (modify 0x00A / 0x00D / 0x051); render.entity | covered | (look / race override in render; packet layouts: 0x00A / 0x00D / 0x051) |
| dynamishelper | Dynamis proc timers and currency | filters.chat; `target_change`; hud.text; actions.lot; timers.add | covered | (font families) |
| EasyNuke | Unified nuke and cure commands | commands.register; actions.cast / commands.run('/ma ...'); filters.typed | covered | - |
| EmpyPopTracker | Pop items and key items for NMs | game.items / key_items; `item_add` / `item_remove`; game.treasure; res.item / key_item; hud.text; net.open_url | covered | - |
| enemybar | Big target HP bar, draggable | hud.bar; hud.text; game.target; `target_change`; input.on_mouse; hud.image | covered | - |
| enternity | Advances NPC dialogue by itself (not choices) | game.event (menu showing); actions.advance; timer.every | covered | - |
| equipviewer | Equipment grid with icons, ammo count, encumbrance | game.equipment / items; hud.icon (item); hud.rect; hud.text; hud.image | covered | (encumbrance flags; packet layouts: 0x01B) |
| eval | Runs Lua from the command line | commands.register; load (text) | covered | - |
| FastCS | Uncaps the frame rate during cutscenes | client.set_option('cutscene_speed', n) while `game.event()` shows; `status_change`; `zone_change` | covered | - |
| ffocolor | Moves FFOChat lines to a chosen mode and colour | filters.chat (new text and mode) | covered | - |
| findAll | Searches every character's bags; on-screen tracker | game.items (all bags) / key_items; fs (shared/); storage.account; ipc.send; hud.text; res.item / key_item | covered | (extdata layouts: storage slips) |
| gametime | Vana'diel time, day, moon, travel times | game.time; `time_change` / `day_change` / `moon_change`; hud.text | covered | - |
| GearSwap | Gear swaps around actions, driven by user job files | filters.typed; packets.on_out / inject_out; actions.equip / cast / ability / ranged / use_item; events; game.*; binds; load (env); fs; client.copy; hud; res.augments; timers.add; the pet entity's mpp / tp; res.message; res.mob_skill; res.weather / weather_element / element | covered | (packet batch order; res monstrosity; res merits; packet layouts: 0x01A / 0x037 / 0x050 / 0x051) |
| GearSwap job-file API | get_sets / precast / midcast / aftercast callbacks, equip, set_combine, state tables | as GearSwap: the callbacks and the player / pet / party / buffactive / world tables built from game.* and events; res.augments; the pet entity's mpp / tp; res.weather / weather_element / element | covered | (packet batch order; res merits) |
| giltracker | Gil HUD | game.gil; `item_change`; hud.text; input.on_key; hud.image | covered | - |
| highlight | Recolours party members' names in chat | filters.chat; game.party; `party_change` | covered | - |
| indinope | Hides Indi- bubbles | packets.on_in (modify 0x00D / 0x037) | covered | (effect bubble override in render; packet layouts: 0x00D / 0x037) |
| InfoBar | Target info bar with a mob database and notes | game.target; `target_change`; game.time; res.zone; hud.text; fs / json in place of its sqlite3 files; res.weather / weather_element / element | covered | - |
| InfoReplacer | %tokens in typed text become game info | filters.typed; game.target / player / items / camera / time / weather; res.item / zone; res.weather / weather_element / element | covered | - |
| instaLS | Linkshell chat right after zoning | filters.typed; packets.on_out (modify 0x0B5); game.items | covered | (packet layouts: 0x0B5) |
| invtracker | Inventory slot grid HUD | game.items / bag_size; `item_add` / `item_remove`; hud.rect; input.on_key; hud.image | covered | - |
| itemizer | Moves items between bags; fetches items for /item | actions.move / sort; filters.typed; game.items / bag_size; res.item | covered | - |
| JobChange | Job change by command at a moogle | actions.talk; packets.on_in (block 0x02E) / inject_out(0x100); game.entities; res.job | covered | (job change action; packet layouts: 0x02E / 0x100) |
| latentchecker | Weapon skill points on trial weapons | actions.equip / move; game.items (extdata) | covered | (extdata layouts: WS points) |
| linker | Web searches in the browser | net.open_url; commands.register | covered | - |
| linuxfix | Wine keybind workaround | binds.list; input.on_key; input.send; not needed on a native client | covered | - |
| Logger | Chat log to a dated file per character | filters.chat; fs.append; os.date; text.plain; `login` | covered | - |
| Lookup | Wiki / auction pages for items and targets | net.open_url; game.entity_by_tag; text.plain; res.item / zone | covered | - |
| lottery | Passes on the other windows when one lots | game.treasure (lotted); ipc.send; actions.pass | covered | - |
| macrochanger | Macro book and set by job | `job_change`; commands.run('/macro book N', '/macro set N') | covered | - |
| MobCompass | Compass around the target for Sneak Attack / GEO | hud.text; game.target; game.player (heading); input.on_mouse | covered | - |
| MountMuzzle | Changes or mutes mount music | packets.on_in (modify 0x00A / 0x05F) / inject_in(0x05F); `zone_change`; audio.music / mute_music | covered | (packet layouts: 0x00A / 0x05F) |
| MountRoulette | Random mount | actions.mount; game.key_items; res.key_item; res.mount | covered | - |
| MyHome | Picks and uses a warp spell or item | actions.cast / use_item / equip; game.spells / recasts / items (extdata); ipc.send | covered | (extdata layouts: charges, use timer) |
| NoCampaignMusic | No campaign music in Shadowreign zones | packets.on_in (modify 0x00A / 0x05F); game.zone; audio.music / mute_music | covered | (packet layouts: 0x00A / 0x05F) |
| Nostrum | Click-to-cure party grid | game.party / target; hud.rect / text / bar + hud.on; input.on_mouse / on_key; actions.cast; hud.image | covered | (font families) |
| NyzulHelper | Nyzul floor, time, objective, tokens | filters.chat; game.key_items / party; hud.text | covered | - |
| obiaway | Fetches and stores elemental obis by day and weather | actions.move; `buff_gain` / `buff_lose` / `day_change` / `weather_change`; game.items / weather; res.weather / weather_element / element | covered | - |
| ohShi | Procs, mob casts and TP moves, rolls | `action`; `battle_message`; filters.chat; res.spell / ability / status; hud.text; res.mob_skill; hud.image | covered | - |
| Omen | Omen objectives window | filters.chat; hud.text; audio.play; `zone_change` | covered | - |
| organizer | Gathers gear for job sets between bags and storage | actions.move / unequip; actions.talk (moogle); game.items / bag_size (extdata); fs; res.item; res.augments | covered | (extdata layouts: slips) |
| PetSchool | Tells Spellcast about pet casts | `action`; mods.emit / mods.call | covered | - |
| PetTP | Pet HP / MP / TP | game.entity_by_tag('pet') (hpp); packets.on_in(0x044 / 0x067 / 0x068); hud.text; the pet entity's mpp / tp | covered | (packet layouts: 0x044 / 0x067 / 0x068) |
| plasmon | Delve plasm, kills, airlixirs | filters.chat; hud.text; timers.add | covered | (font families) |
| plugin_manager | Per-character add-on sets | `login`; commands.run('/lua load ...', '/lua unload ...') | covered | - |
| pointwatch | XP / CP / JP and Dynamis time | `battle_message`; game.stats / job_points / key_items; filters.chat; packets.on_in(0x02A / 0x110); hud.text | covered | (dialogue text by id; packet layouts: 0x02A / 0x110) |
| porter | Storage slip contents | game.items (extdata); res.item | covered | (extdata layouts: slips) |
| position_manager | Game window position per character | client.window / set_window; ipc.send; client.set_window | covered | - |
| Pouches | Uses every copy of an item | actions.use_item; game.items; res.item; sleep | covered | - |
| RAWR | Sound when the Unity leader mentions dragons | filters.chat; audio.play | covered | - |
| reive | Reive score and bonuses | filters.chat; `buff_gain` / `buff_lose`; hud.text | covered | (font families) |
| remember | Re-requests entities that failed to spawn | packets.on_in(0x00D / 0x00E); packets.inject_out(0x016); the client already re-asks (c2s 0x017) | covered | (packet layouts: 0x00D / 0x00E / 0x016) |
| respond | //r replies to the last tell | `chat_message`; commands.register; commands.run('/t ...') | covered | - |
| Rhombus | Clickable on-screen action menu | hud.rect / text + hud.on; input.on_mouse / on_key; game.abilities / spells / job_data; res.spell / ability; commands.run | covered | - |
| roe | Records of Eminence objective profiles | packets.on_in(0x111 / 0x112); packets.inject_out(0x10C / 0x10D); settings | covered | (RoE objectives in game; packet layouts: 0x111 / 0x112 / 0x10C / 0x10D) |
| rolltracker | COR roll bonuses, bust odds, no double-up on lucky rolls | `action`; game.party (buffs); filters.chat; filters.typed; game.equipment; res.status / ability | covered | - |
| salvage2 | Salvage pathos and zone timer | filters.chat; hud.text; `zone_change`; timers.add | covered | - |
| SATACast | Tells Spellcast about Sneak / Trick Attack | `buff_lose`; `action`; mods.emit | covered | - |
| scoreboard | Live DPS parser | `action` / `damage`; game.party / entity; hud.text; fs.write | covered | - |
| send | Runs commands on other game windows | ipc.send / `ipc_message`; commands.run; game.party | covered | - |
| setbgm | Sets any music | packets.inject_in(0x05F); audio.music / mute_music | covered | (packet layouts: 0x05F) |
| SetTarget | Target by id | actions.target | covered | - |
| shortcuts | Spellcast-style command and target completion | filters.typed; game.entities / spells / abilities / job_data / key_items; res.spell / ability; res.mount | covered | (res monstrosity; packet layouts: 0x0AE) |
| shoutHelper | Editable alliance job list | ui.window / hud.text; commands.register | covered | - |
| Silence | Drops repeated equipment-change lines | filters.chat; filters.typed | covered | - |
| SpeedChecker | Movement speed modifier | game.entity (speed); hud.text | covered | - |
| SpellBook | Unknown spells by job or category | game.spells; res.spell (levels); res.job | covered | - |
| SpellCheck | Unlearned spells | game.spells; res.spell | covered | - |
| stna | One-button status removal for dual boxing | `buff_gain` / `buff_lose`; ipc.send; commands.run; res.status | covered | - |
| stopwatch | Count-up timers | hud.text; clock; timer.every | covered | - |
| StratHelper | SCH stratagem count for Spellcast | game.recasts; game.job_data; `job_change`; `action`; mods.emit | covered | - |
| Stubborn | Blocks accidental calls for help | packets.on_out (block 0x01A) / inject_out | covered | (packet layouts: 0x01A) |
| SubTarget | `<stal>` pick as the target of a send command | commands.register; game.target (after /target <stal>); ipc.send | covered | (`<lastst>` target tag) |
| switch_focus | Switches OS focus between game windows | ipc.send; client.focus; `focus_change` | covered | - |
| Tab | Tab to `<stnpc>`, X+Tab to `<stpc>` | input.on_key / held; commands.run('/ta <stnpc>') | covered | - |
| targetinfo | Target id, index and flags | game.target; hud.text | covered | - |
| temps | Buys Escha temporary items | actions.talk; packets.on_in(0x034 / 0x05C) / inject_out(0x05B); game.items | covered | (packet layouts: 0x034 / 0x05B / 0x05C) |
| Text | On-screen text objects from commands | hud.text / set / remove; commands.register | covered | (font families) |
| thtracker | Treasure Hunter level | `action` (proc messages); `battle_message`; `target_change`; hud.text | covered | - |
| Tickle | Resting tick countdown | `status_change`; `hp_change` / `mp_change`; hud.text; timer.every | covered | - |
| timestamp | Time stamps on every line | filters.chat; os.date | covered | - |
| TParty | Target HP% and party TP beside the party list | game.party / target; hud.text; hud.screen | covered | (native window bounds) |
| translate | Rough JP to EN translation of chat | filters.chat; `chat_message`; text.from_shift_jis / plain; res.item / spell; res.autotranslate; text.autotranslate / phrase; res.name(kind, id, 'ja') | covered | - |
| TreasurePool | Pool window with drop timers and lots | game.treasure; packets.on_in(0x0D2); hud.text | covered | (treasure drop time; packet layouts: 0x0D2) |
| Treasury | Lots, passes and drops by list; stacks | game.treasure; actions.lot / pass / drop / sort; ipc.send; res.item | covered | - |
| Trusts | Saved trust sets | actions.cast; commands.run('/retr ...'); game.party / spells / recasts / key_items; res.spell | covered | - |
| update | Updates Windower and reloads add-ons | the client's Update button; commands.run('/lua reload'); ipc.send; client.run | covered | - |
| VisibleFavor | Shows the avatar's Favor / Ward range | draw.ring; game.entity_by_tag('pet') | covered | (effect bubble override in render) |
| vwhl | Weakness lines to the tell stream, highlighted | filters.chat (new text and mode) | covered | - |
| widescan | Wide Scan search and tracking by name | actions.scan / track / untrack; game.npc_names / entities; packets.on_in(0x0F4 / 0x0F6) | covered | (Wide Scan results; packet layouts: 0x0F4 / 0x0F6) |
| xivbar | HP / MP / TP bars at screen centre | hud.bar (hp / mp / tp); game.player; vitals events; hud.text; hud.image | covered | - |
| Yush | File-based key-sequence macro engine | input.on_key (taken); load (env); fs.read; commands.run; hud.text; client.focus; `focus_change` | covered | - |
| zonetimer | Time in zone | `zone_change`; hud.text; clock | covered | - |

### Windower plugins

| Add-on | What it does | API used | Status | Missing |
| --- | --- | --- | --- | --- |
| AutoExec | Runs commands on events (login, job, time, chat, buffs, zone, weather, HP thresholds, no ammo, examined); wildcards, run once | `events.on` (login, job_change, time_change, chat_message, buff_gain / lose, zone_change, weather_change, hpp_change), `filters.chat`, `game.equipment`, `commands.run`; `emote` / `party_invite` / `examined` | covered | - |
| Binder | G-keys, Stream Deck and similar devices fire aliases | `binds.set` / `input.on_key` for devices set to send keys; `input.on_mouse` back / forward; devices.list / open / write (raw HID reports) | covered | (gamepad input, deferred) |
| ChatMon | Sound alerts on tells, name mentions, emotes, examines, invites | `chat_message`, `filters.chat` (system lines), `audio.play` / `play_se` | covered | - |
| Config | Engine settings: aspect, animation rate, draw distance, footsteps, frame rate divisor, auto target | native Settings (frame limit, Draw Distance, 3D aspect); `ui.native.set_config` for game options; client.set_option(animation_rate) | covered | - |
| DelayMeNot | Removes the macro palette's display delay | client.set_option('macro_palette_delay', false) | covered | - |
| FFXIDB | Minimap: drag, zoom, opacity, kind toggles, range circles, target lines | native Minimap Extra (zoom, entity dots); draw.ring / line; hud.rect / image; game.map / map_pos; hud.map | covered | - |
| FishingCrashFix | Patches an old client crash | not applicable (our code) | covered | - |
| Guildwork | Syncs character data with a guild site | `net.request`; `game.items` / `stats` / `jobs` | covered | - |
| IME | Japanese IME input in the chat line | native: the chat line takes the system IME (composition at the caret, commits, the candidate window there); text.to_shift_jis | covered | - |
| InfoBar | Target info bar | `game.target`, `target_change`, `hud.text` | covered | - |
| Run | Starts / stops external programs | client.run | covered | (stopping a started program) |
| Sandbox | Per-instance copies of shared files for multiboxing | native: `FFXI_CONFIG` per instance; `storage.character` | covered | - |
| SSOrganizer | Files screenshots by character and zone | client.screenshot (subfolders by character and zone); game.zone | covered | - |
| Tickle | Time to the next resting tick | `status_change`, `hp_change`, `timer.every`, `hud.text` | covered | - |
| Timers | Recast / buff bars with icons; custom timers by command; a service other add-ons call | native Timers Extra; timers.add / remove (rows in the Timers window, with icons); game.recasts / buffs; commands.register; mods.provide | covered | - |
| WeatherMon | Weather alerts with sound | `weather_change`, `audio.play`, `chat.add` | covered | - |
| WinControl | Moves / resizes the window, always on top | client.window / set_window (move, size, top) | covered | - |

### Windower deprecated plugins

| Add-on | What it does | API used | Status | Missing |
| --- | --- | --- | --- | --- |
| Attainment | XP / merit tracking (deprecated, PointWatch) | `game.stats` (exp, limit, merits), `game.job_points`, `battle_message`, `hud.text` | covered | - |
| BlinkMeNot | Stops gear-swap model blinking (deprecated, DressUp) | packets.on_in holds or rewrites 0x051 / 0x00D (raw): every player look comes from 0x00A / 0x00D / 0x051 | covered | (look / race override in render; packet layouts) |
| BoxHelper | Casket solving (deprecated, BoxDestroyer) | `filters.chat` (hint lines), `game.target`, `chat.add` | covered | - |
| Cancel | Cancels buffs by name (deprecated, Cancel) | `actions.cancel`, `res.status` | covered | - |
| ChatLink | Opens / copies URLs from chat (deprecated, ChatLink) | `filters.chat`, `net.open_url`, `client.copy` | covered | - |
| Clock | Clock overlay (deprecated, Clock) | native Clock Extra; `os.date`, `hud.text` | covered | - |
| ConsoleBG | Console background (deprecated, ConsoleBG) | not applicable (no console); `hud.rect` | covered | - |
| Distance | Distance overlay (deprecated, Distance) | native target distance Extra; `Entity.distance`, `hud.text` | covered | - |
| DrawDistance | Engine draw distance (deprecated, Config) | native Draw Distance setting | covered | - |
| FFOChat | Third-party chat network over sockets (offline) | `net.request` (HTTP polling), `chat.add`; net.connect / listen / udp | covered | - |
| Find | Item search across characters (deprecated, FindAll) | native `/find`; `game.items`, `storage.account`, `fs` shared area | covered | - |
| GearCollector | Gathers job gear from storage (deprecated, Organizer) | `actions.move`, `game.items`, raw `Item.extdata` compare; res.augments | covered | - |
| Itemizer | Moves items between bags, fetches before `/item` (deprecated, Itemizer) | `actions.move`, `filters.typed`, `res.item` | covered | - |
| LightLuggage | Lot / pass / drop profiles (deprecated, Treasury) | `game.treasure`, `actions.lot` / `pass` / `drop`, `item_add` | covered | - |
| Logger | Chat log to file (deprecated, Logger) | `filters.chat`, `fs.append` (`files.shared`) | covered | - |
| PetTP | Pet HP / MP / TP (deprecated, PetTP) | `game.info().pet_index`, `Entity.hpp`; 0x067 / 0x068 raw; the pet entity's mpp / tp | covered | - |
| PlasticSurgeon | Changes entity race, face, gear (deprecated, DressUp) | `packets.on_in` rewrite of 0x00A / 0x00D / 0x051 (raw); `render.entity` scale / tint | covered | (look / race override in render; packet layouts) |
| Silence | Suppresses messages (deprecated, Silence) | `filters.chat` | covered | - |
| Spellcast | XML rules for gear and commands (deprecated, GearSwap) | `filters.typed`, `actions.equip` / `cast`, `action` event, `packets.on_out`, text `load` | covered | (packet batch order) |
| Text | Text objects by command (deprecated, Text) | `hud.text`, `commands.register` | covered | (font families) |
| Timestamp | Chat timestamps (deprecated, Timestamp) | native timestamps Extra; `filters.chat`, `os.date` | covered | - |
| TParty | Party HP% / TP overlay (deprecated, TParty) | native party TP; `game.party`, `hud.text` | covered | - |
| Unlimited | Removes the frame cap (deprecated, Config) | native frame limit (default 300) | covered | - |
| UpdateResources | Resource updates (deprecated, launcher) | `res` from the game files; native updater | covered | - |
| VanaTunes | Music control (deprecated, SetBGM) | `packets.inject_in` / `on_in` 0x05F (raw); audio.music / mute_music | covered | - |
| ZoneTimer | Time in zone (deprecated, ZoneTimer) | `zone_change`, `clock`, `hud.text` | covered | - |

### Windower features

| Add-on | What it does | API used | Status | Missing |
| --- | --- | --- | --- | --- |
| %-variables | `%area`, `%target` ... expanded in commands | `filters.typed` + `game.*` | covered | - |
| Binds and aliases | `bind` / `unbind` / `listbinds` with modifiers, chat state, release; `alias` | `binds.set` (`^!~@`, up, typing) / `unset` / `list`, `input.typing`; aliases as `commands.register` + `commands.run` | covered | - |
| Console | Console: echo, font, colour, fade, log to file, `exec` scripts, `lua exec` | `print` / `chat.add` to the log, `//` commands on the chat line, `/lua run`; script files by `fs.read` + `commands.run` | covered | - |
| Input commands | `input`, `type`, `paste`, `setkey` down / up, block keyboard / mouse | `commands.run`, `chat.set_line`, `input.send` / `press`, `input.on_key` / `on_mouse` returning true; client.paste | covered | - |
| Launcher profiles | Resolution, UI scale, super sampling, window mode, sound, bump / mip mapping, gamma, which add-ons load | native Settings screen, Mods page, `FFXI_CONFIG` per instance | covered | - |
| Pipes and MMF input | Named pipes and memory-mapped key input for external programs | net.listen (a local TCP port for external programs); commands.run; input.press; net.pipe | covered | - |
| Render knobs | Wireframe, gamma, forced ambient light | native Gamma setting; client.set_option(ambient / wireframe) | covered | - |
| Screenshots | `screenshot` bmp / jpg / png, optionally without overlays | native screenshot key (PNG, interface hidden); client.screenshot | covered | - |
| showfps | FPS counter | `frame` event `dt` + `hud.text`; F12 overlay | covered | - |
| Video capture | `video start` / `stop` | client.record / stop_recording | covered | - |
| Window and process | Minimise, exit, terminate, resize, frame, full screen | `commands.run('/shutdown')`; client.window / set_window (move, size, minimised, fullscreen, border) | covered | - |

### Windower community add-ons

| Add-on | What it does | API used | Status | Missing |
| --- | --- | --- | --- | --- |
| AllSeeingEye / truesight | Shows hidden NPCs | `packets.on_in` 0x00E flag rewrite, 0x038 block (raw, as they do) | covered | - |
| anchor | Removes knockback | `packets.on_in(0x028)` bit rewrite (raw, as it does) | covered | - |
| AuctionHelper | Auction House buy / sell, bazaar price | `packets.inject_out` / `inject_in`, `packets.last('out', id)` | covered | (packet layouts: 0x04E / 0x04B / 0x10A) |
| AutoCOR | Auto Phantom Roll | `action` event, `game.buffs`, `actions.ability`, `game.player().pos` | covered | - |
| AutoGEO | Auto Geo / Indi | `actions.cast`, `action` event, `game.buffs` | covered | - |
| autoMB | Auto magic burst | `action` event, `actions.cast` with a target, `game.time` / `weather` | covered | - |
| Balloon | NPC dialogue in a story balloon | `filters.chat` (dialogue modes), `game.event`, `input.on_key`, `hud.text` / `rect`, `game.info().chat_open`; hud.image | covered | (font families) |
| buddypal | Multibox desktop app over a DLL | `packets.on_in` / `on_out`, events, `net.request` + text `load`; net.connect / listen / udp | covered | (WebSocket framing) |
| CurePlease_addon | Companion for a healer-bot program | `game.party` buffs, `action` event; net.connect / listen / udp | covered | - |
| enemybar2 | Large target HP bar with action text | `game.target`, `hud.bar` / `text`, `action` event, `input.on_mouse`; hud.image; res.mob_skill | covered | - |
| FastFollow | Tight multibox follow, across zones | `ipc.send` / `ipc_message`, `actions.run` / `run_to` / `follow` / `stop`, `frame` | covered | - |
| FFXI Rich Presence | Discord presence through a helper program | `fs.write` (state JSON); client.run | covered | - |
| fisher | Fishing bot | `packets.on_in` 0x115, `packets.inject_out` 0x110, `actions.equip` / `move`, `commands.run('/fish')` | covered | (packet layouts: 0x110 / 0x115) |
| Gaze_check | Turns away from gaze attacks | `action` event (a move list in the mod), `actions.turn` | covered | - |
| GearInfo | Gear stat totals fed to GearSwap | `game.equipment`, `res.item` descriptions, `game.stats` / `buffs`, `hud.text` / `bounds`, `mods.call`; res.augments; hud.image | covered | - |
| GearSwap framework libs | GearSwap framework: auto buffs, weapon skills, displays | GearSwap port: `filters.typed`, `actions.equip`, `packets.*`, `json`, `actions.turn`, `commands.run('/macro book')`; res.augments; timers.add; res.mob_skill | covered | (packet batch order) |
| GEO-HUD | Geomancer HUD plus ground rings (DLL) | `draw.ring` (replaces the DLL), `game.entities` (luopan), `game.party` buffs, `hud`; hud.image | covered | - |
| HealBot | Auto cure / buff / debuff, assist, follow | `action` / `battle_message`, `game.party` buffs, `actions.cast` / `assist` / `follow` / `run_to` / `stop`, `ipc.send`; res.message; res.mob_skill | covered | - |
| Helper | Voiced in-game reminders | `audio.play` (WAV), events, `net.request` + `fs.write` (downloads); hud.image | covered | - |
| htmb | Buys HTMB key items | `packets.build` / `inject_out` (raw, as it does), `ipc.send` | covered | - |
| Lazy | Farming bot | `action` event, `game.entities`, `actions.attack` / `run_to` / `turn` / `follow` | covered | - |
| LLMChat | LLM-written chat replies | `chat_message`, `net.request`, `chat.set_line` / `commands.run` | covered | - |
| MAGA (Oseem) | Rolls Oseem augments until a match | `packets.*`, raw `Item.extdata`; res.augments | covered | (packet layouts) |
| Mote-based GearSwap job files | Job files on the Mote libs | GearSwap port: `binds.set`, `hpp_change`, `commands.run('/macro book')`; timers.add; res.augments | covered | (packet batch order) |
| NpcInteract | Alts copy the main character's NPC interactions | `packets.on_out`, `ipc.send`, `packets.inject_out` | covered | (packet layouts) |
| OmniWatch | External overlay fed over UDP | `fs.mkdir`, `actions.drop`, `packets.*`; net.connect / listen / udp | covered | (packet layouts: ~60 ids) |
| Parse | Damage / accuracy parser with export | `action` / `damage` / `battle_message`, `hud.text`, `fs.write`, `input.on_mouse`; res.message; res.mob_skill | covered | - |
| PartyBuffs | Buff icons beside the party list | native party status icons; `game.party` buffs, `hud.icon` | covered | - |
| PorterPacker | Job gear in / out of Porter Moogle slips | `actions.trade`, `packets.*`, raw `Item.extdata` | covered | (extdata layouts: slips; packet layouts) |
| Pricer | Price lookup on a web site | `net.request`, `commands.register`, `chat.add` | covered | - |
| questmarks | Quest markers over NPCs in the 3D world | `draw.to_screen` + `hud.text`, `game.npc_names` / `entities`; hud.image; game.map / map_pos; hud.map | covered | (quest / mission state in game) |
| React | Reactions to monster moves (face away, run, gear) | `action` event, `actions.turn` / `run` / `equip`, `commands.run`; res.mob_skill; res.skill | covered | - |
| renamer | Renames entities | `render.entity` name | covered | - |
| revisible | Invisible players drawn translucent (DLL) | `packets.on_in` 0x00D / 0x037 flag rewrite (raw) + `render.entity` alpha | covered | - |
| Roller | Auto Corsair rolls | `action` event, `actions.ability`, `zone_change` | covered | - |
| RuptChat | Replacement chat log (tabs, search, archive) | `filters.chat`, `chat.line`, `input.on_mouse` (wheel), ui.window (list, input, tabs), `fs.append`; ui.native.hide('log') | covered | (font families) |
| SellNPC | Sells items by name | `actions.sell`, `game.items`, `res.item` | covered | - |
| SheolHelper | Odyssey maps, segments, mob weaknesses | `hud.text`, `filters.chat`, `game.entities`, `packets.on_in`; hud.image | covered | (packet layouts) |
| shout_ml | Machine-learning yell spam filter | `filters.chat` (hold), `net.request`, `chat.add` with the mode to restore | covered | - |
| Silmaril | Multibox controller driven by a companion program | `ipc.send`, `packets.inject_in` (0x065 is decoded), `actions.run` / `turn`; net.connect / listen / udp; timers.add; res.mob_skill | covered | - |
| Singer | Auto Bard songs | `actions.cast`, `action` event, `game.party` buffs; timers.add | covered | - |
| Skillchains | Live skillchain / burst window | native skillchain helper Extra; `action` event, `hud.text`; property data shipped with the mod | covered | - |
| SmartSkillup | Automated magic skill-ups with a GUI | `game.skills`, `actions.cast`, `ui.window` / `hud`, `hud.bounds` | covered | - |
| SortieBuddy | Sortie NM / blitzer locations | `packets.inject_out` 0x016 (raw), `game.entity` / `npc_names`, `draw` / `hud` | covered | - |
| Sparks | Buys Sparks items by command | `actions.talk` / `choose`, `packets.inject_out` / `on_in` | covered | (packet layouts: 0x034 / 0x05B) |
| SuperWarp | Warps by command (Home Points, waypoints, Survival Guides, Escha, Odyssey) | `actions.talk`, `game.event` / `actions.choose`, `packets.inject_out` / `on_in`, `ipc.send`, `actions.stop` | covered | (packet layouts: 0x034 / 0x05B / 0x05C) |
| TradeNPC | Trades items to an NPC by command | `actions.trade` | covered | - |
| Trust | AI plays the character: heal, buff, pull, skillchain, follow, multibox | `actions.*` (cast, trade, run, turn, follow), `chat.line` / `set_line`, `ipc.send`, `net.request`, `fs.list`, `audio.play`, `ui.window`; hud.image; res.mob_skill | covered | (packet layouts: ~25 ids; SQLite) |
| UnityNPC | Buys Unity rewards | `actions.talk` / `choose`, `packets.inject_out` / `on_in` | covered | (packet layouts: 0x034 / 0x05B) |
| XICamera | Camera distance and pan speeds (memory patch) | render.set_camera {distance, speed} | covered | - |
| XIchecklist | Mission / quest / key item / RoE checklist | `game.key_items`, `res.key_item`, `ui.window`, `client.copy` | covered | (quest / mission state in game) |
| XIPivot | DAT / sound overlay loader (native module) | native mod manager (DAT and sound overlays) | covered | - |
| XIVCrossbar | FFXIV-style gamepad crossbar | `input.on_key` (take), `binds.set`, `hud.icon` (item icons), `mp_change`; hud.image | partly | gamepad input (deferred) |
| XIVHotbar / XIVHotbar2 | Keyboard hotbars | `binds.set`, `hud.on`, `hud.icon`, `game.recasts`, `mp_change`; hud.image | covered | - |
| XivParty | FFXIV-style party list with buffs | `game.party`, `hud.bar` / `icon` / `text`, `input.on_mouse`, `hud.on`, `ui.native.hide('party')`; hud.image | covered | - |

### Windower community capabilities

Capabilities several Windower community add-ons need, each rated as one row.

| Add-on | What it does | API used | Status | Missing |
| --- | --- | --- | --- | --- |
| Capability: chat input line | Read / write the typed line | `chat.line`, `chat.set_line` | covered | - |
| Capability: drawing anchored in the 3D world | World-to-screen markers, ground rings | `draw.line` / `ring` / `box`, `draw.to_screen`, `game.camera` | covered | - |
| Capability: entity name override | `set_mob_name` every frame | `render.entity` name | covered | - |
| Capability: gamepad input | Pad buttons as key chords (XIVCrossbar) | none (gamepad support deferred) | missing | gamepad input (deferred) |
| Capability: moving the character | `run` / `turn` / `follow` / walk (FastFollow, HealBot, Lazy, React, Trust ...) | `actions.run` / `run_to` / `turn` / `follow` / `walk` / `stop` | covered | - |
| Capability: native code and raw memory | DLLs / memory modules: camera patch, file redirection, scene rings, entity translucency, packet hooks | render.set_camera (distance, pan speed), native DAT overlays, `draw.ring`, `render.entity`, `packets.*` | covered | - |
| Capability: our position set client-side | Injected 0x065 moves our character | `packets.inject_in` (0x065 is decoded) | covered | - |
| Capability: outbound HTTP(S) | Price lookups, LLM calls, update checks | `net.request` | covered | - |
| Capability: running downloaded code | Fetch a Lua bundle and run it | `net.request` + text `load` | covered | - |
| Capability: shell and process access | Directory listing, mkdir, curl, start a companion program | `fs.list` / `mkdir`, `net.request`; client.run | covered | - |
| Capability: small API gaps | Text extents / location / border, `mp change` / `hpp change`, `drop_item`, `last_outgoing`, map calibration, item augments | `hud.bounds`, `hud.text` pad, `mp_change` / `hpp_change`, `actions.drop`, `packets.last('out', id)`; res.augments; game.map / map_pos; hud.map | covered | - |
| Capability: sockets to companion programs | UDP to companion programs; sync between instances | `ipc.send` between game windows; net.connect / listen / udp | covered | - |
| Capability: Timers as a service | `timers c` / `d` commands from other add-ons (Singer, Mote-based job files, Silmaril) | timers.add / remove (rows in the Timers window); commands.register for the `timers` commands | covered | - |

### Ashita v4 official add-ons

| Add-on | What it does | API used | Status | Missing |
| --- | --- | --- | --- | --- |
| actionparse | Shows decoded action packets in a table; copies to the clipboard | event `action`; ui.window (list, columns, tips); client.copy | covered | - |
| activemon | Image showing whether this game window has focus | hud.icon (game icons only); client.focus; `focus_change`; hud.image | covered | - |
| affinity | Sets the process CPU affinity | client.set_affinity / set_priority / process (permission process) | covered | - |
| ahcolors | Recolours AH list rows (learned / learnable / unlearnable) | ui.native.item_colour; res.item; game spells known | covered | - |
| ahgo | Opens the AH anywhere (fake s2c 0x04C); moving with it open | packets.inject_in(0x04C) opens our AH; client.set_option('menu_reach', false) | covered | - |
| allmaps | Every map in /map without the key items | packets.on_in(0x055) can fake the map key items (they then show in Key Items too); client.set_option('all_maps', true) | covered | - |
| aspect | 3D aspect matches the window | native: the projection keeps the window's aspect | covered | - |
| autojoin | Answers party invites automatically | packets.on_in(0x0DC); actions.join / decline | covered | (packet layouts) |
| autologin | Logs into a chosen character slot | mod.toml `lobby`; lobby.state / sign_in / play; `lobby_screen`; storage.seal / unseal | covered | - |
| autorespond | AFK auto-reply to tells | event `chat_message` (kind 3); commands.run | covered | - |
| blucheck | Learned BLU spells in a tabbed window | game.spells; res.spell; ui.window (list / buttons, tabs); net.open_url | covered | - |
| blumon | Announces newly learned BLU spells | event `battle_message`; res.spell; chat.add | covered | - |
| blusets | Saves / loads BLU spell sets by command | game.job_data; actions.blue_set / blue_remove; fs | covered | - |
| cartographer | Every map in the map menus for other zones | packets.on_in(0x055) can fake the map key items (they then show in Key Items too); client.set_option('all_maps', true) | covered | - |
| casper | Walk through other players | native: idle players never block, moving ones hold 0.5 s; client.set_option('player_collision', false) | covered | - |
| cfhblock | Blocks call for help | packets.on_out(0x01A) returning false | covered | (packet layouts) |
| chamcham | Tints models by entity type; colour editor | render.entity tint; game.entities; ui.window (colour) | covered | - |
| changecall | Swaps the call id in typed and received lines | filters.typed; filters.chat | covered | - |
| chatfix | Works around a chat packet mismatch | not needed natively; packets.on_in / on_out if ever | covered | - |
| chatmon | Sounds on chat events (tells, invites, emotes...) | filters.chat; event `chat_message`; packets.on_in; audio.play; ui.window (input) | covered | - |
| checker | Richer /check text on monsters | packets.on_in(0x029, 0x0F4), blocking; chat.add | covered | (packet layouts) |
| chime | Plays the game's chime | audio.play_se | covered | - |
| cleancs | Hides add-on drawing in cutscenes | game.event / game.info().event; hud.set visible | covered | - |
| clearcolor | Sets the scene's background colour | ui.window (colour); client.set_option(clear_colour) | covered | - |
| clock | Earth and Vana'diel clocks on screen | native Clock extra; game.time; os.date; hud.text | covered | (font families) |
| config | Sets any game config value by command | ui.native.config / set_config | covered | - |
| craftmon | Synthesis result as soon as the synth starts | packets.on_in(0x030, 0x06F); res.item; chat.add | covered | (packet layouts) |
| crosshair | Screen guide lines for placing UI | hud.rect (1 px); input.on_mouse | covered | - |
| debuff | Cancels a status effect by command | actions.cancel | covered | - |
| distance | Distance to the target on screen | native target distance extra; game.target; hud.text | covered | (font families) |
| drawdistance | Scene and mob draw distance by command | native Draw Distance setting | covered | (engine settings from Lua) |
| enternity | No Enter needed through NPC text | actions.advance; game.event | covered | - |
| equipmon | Equipment grid on screen; config window | game.equipment; hud.icon / rect; input.on_mouse; hud.image; ui.window (colour, tips) | covered | - |
| fastswap | Fixes a client state bug on fast job swaps | none (not checked whether ours has the bug) | missing | native state fix |
| filterless | Turns the bad-language filter off | native: no profanity mask is applied yet | covered | - |
| filters | Saves / loads chat filter sets | ui.native.config / set_config; packets.inject_in(0x0B4); fs | covered | - |
| filterscan | Filters Wide Scan results | packets.on_in(0x0F4), blocking; fs | covered | (packet layouts) |
| fps | Shows and sets the frame rate | native frame limit (default 300) and F12; event `frame` dt | covered | (engine settings from Lua) |
| freemem | Trims the process working set | not needed natively | covered | - |
| gateway | Every door drawn open | packets.on_in(0x00E) edit | covered | (packet layouts) |
| hideconsole | Hides the boot console | not needed: no console window | covered | - |
| hideobs | Hides the game window from screen capture | client.set_window {capture = false} (where the system allows it) | covered | - |
| hideparty | Hides the party lists | ui.native.hide('party') | covered | - |
| hideui | Hides every add-on's drawing | its elements: hud.set / ui.close; everything: the UI-hide key; /lua hide | covered | - |
| ime | Japanese IME and character sets in chat | native: the chat line takes the system IME (composition at the caret, commits, the candidate window there); text.to_shift_jis | covered | - |
| imguistyle | Per-character ImGui style | not needed: mod windows follow the active UI theme (by design) | covered | - |
| instantah | No delay on AH results | client.set_option('ah_result_delay', false) | covered | - |
| instantchat | No delay adding chat lines | client.set_option('log_slide', false) | covered | - |
| invmon | Bag space on screen; tabbed config | game.items / bag_size; hud.text; ui.window (tabs, columns, colour, tips) | covered | - |
| itemwatch | Items and key items tracked on screen; editor | game.items / key_items; res.item / key_item; hud; ui.window (input, choice, tabs, colour, tips); ui.prompt / menu | covered | - |
| links | Collects URLs from chat into a window | filters.chat; ui.window buttons; net.open_url | covered | - |
| logincmd | Per-character script at login | event `login`; fs.read; commands.run | covered | - |
| logs | Chat log to files | filters.chat (lines buffered); text.plain; timer.every + fs.append | covered | - |
| macrofix | No macro bar delay on Ctrl / Alt | client.set_option('macro_palette_delay', false) | covered | - |
| mapdot | Enemy dots on the compass for every job | a radar: game.entities + hud.rect; client.set_option('compass_monsters', true) | covered | - |
| minimapmon | Hides the minimap when standing still | native minimap; ui.native.hide('minimap'); game.player().pos | covered | - |
| mipmap | Undoes a later mipmap change in the client | not needed: the renderer builds the mips | covered | - |
| move | Window position, size, border | client.window / set_window (position, size, border) | covered | - |
| namecolors | Edits the name colour table | render.entity name_colour (per entity, by kind / party); ui.window (colour) | covered | - |
| nokb | No knockback on us | packets.on_in(0x028) edit | covered | (packet layouts) |
| nomad | Mog House menu in any zone | packets.inject_in(0x02E) opens our moogle menu; client.set_option('menu_reach', false); actions.open_menu('moogle') | covered | - |
| noname | Hides our name | render.entity(our id, {name = ''}) (inferred: an empty plate) | covered | - |
| onevent | Commands on chat triggers | filters.chat / event `chat_message`; commands.run | covered | - |
| paranormal | Commands while dead | client.set_option('dead_commands', true) | covered | - |
| peekaboo | Shows every entity the client knows | packets.on_in(0x00D, 0x00E) edit | covered | (packet layouts) |
| petinfo | Pet HP / MP / TP and its target | game.info().pet_index; game.entity; packets.on_in(0x068); hud.bar or ui gauges | covered | (packet layouts) |
| quicksets | No delay between equip sets | client.set_option('equipset_delay', false) | covered | - |
| recast | Recast timers on screen | game.recasts; res.ability / spell; hud; native Timers extra | covered | (font families) |
| renamer | Renames entities; editor window | render.entity name; game.npc_names; ui.window (columns, tabs, input, tips); ui.prompt / menu | covered | - |
| repeater | Repeats a command on a timer | timer.every; commands.run | covered | - |
| sexchange | Our race and hair by command | packets.on_in(0x00A, 0x00D, 0x051) edit | covered | (packet layouts) |
| singlerace | Every model one race / hair | packets.on_in(0x00D, 0x00E) edit | covered | (packet layouts) |
| skeletonkey | Forces the targeted door open | packets.inject_in of a door-state 0x00E | covered | (packet layouts) |
| stepdialog | Advances dialogue by command | actions.advance | covered | - |
| stfu | Drops repeated system lines and call sounds | filters.chat | covered | - |
| timestamp | Timestamps on chat lines | native timestamps extra; filters.chat | covered | - |
| tokens | More tokens in typed text (`<hp>`, `<job>`...) | filters.typed with state cached from events (filters cannot read state) | covered | - |
| tparty | Party TP and target HP% | native party TP; game.party / target; hud.text | covered | (font families) |
| trimspells | Shortcut spell list trimmed to known spells | none | missing | native list options |
| truesight | No entity occlusion; invisible players half-transparent | packets.on_in flag rewrite of 0x00D / 0x037 and render.entity alpha (as revisible); client.set_option('occlusion', false) | covered | - |
| watchdog | Wide Scan tracking of any entity by command | game.npc_names; actions.track | covered | - |

### Ashita v3 official add-ons

| Add-on | What it does | API used | Status | Missing |
| --- | --- | --- | --- | --- |
| antiemote | Blocks other players' emotes | packets.on_in(0x05A) returning false | covered | - |
| find | /find items in our bags | native /find extra; game.items; res.item | covered | - |
| findall | Every character's bags cached and searched | game.items; fs under shared/ (files.shared) | covered | - |
| gmcommand | `@` / `$` lines into GM commands | filters.typed | covered | - |
| ibar | Info bar: zone, us, target (bundled mob DB) | hud.text; game.*; res.zone; fs.read | covered | (font families) |
| lotomatic | Lot / pass / move by a rules file | game.treasure; actions.lot / pass / move / drop; fs | covered | - |
| mountmuzzle | Replaces or mutes the mount music | packets.on_in(0x05F) where the server sends it; audio.music / mute_music | covered | - |
| pbar | Player stats text bar | hud.text; game.player / stats | covered | (font families) |
| pupatt | Saves / loads automaton attachment sets | game.job_data; actions.attach / detach; storage | covered | - |
| status | Party members' status icons | game.party buffs; hud.icon | covered | - |
| synplicity | Repeats synths, tracks results | packets.inject_out(0x096); packets.on_in(0x030, 0x06F); hud | covered | (packet layouts) |
| yield | Gathering tracker: window, sounds, reports | events; ui.window (tips); audio.play; fs; net.request; hud.image; client.run | covered | - |
| zoom | Camera zoom range | render.set_camera {distance, max_distance} (up to 10) | covered | - |

### Ashita v4 plugins

| Add-on | What it does | API used | Status | Missing |
| --- | --- | --- | --- | --- |
| addons | Hosts Lua add-ons | the mod runtime (a VM per mod) | covered | - |
| extraslots | More stored accounts at sign-in | native: the login form keeps every account used in its account list, passwords sealed | covered | - |
| hardwaremouse | Forces a hardware cursor | not needed natively | covered | - |
| minimap | Themed minimap overlay | native Minimap extra; ui.native.hide('minimap'); hud.image; game.map / map_pos; hud.map | covered | - |
| quicky | Faster launcher sign-in | not needed: the client's login screen | covered | - |
| sandbox | Runs POL / FFXI without an install (registry, COM) | not needed natively | covered | - |
| screenshot | Back-buffer screenshots, add-on UI optional | native Print Screen (with the UI hidden only); client.screenshot | covered | - |
| thirdparty | External programs send commands, keys, text | net.request polling a local HTTP tool; commands.run; input.press; net.connect / listen / udp | covered | - |
| toon | Cel shading with outlines | render.shader (a WGSL pass: posterize, depth outlines) / shader_params | covered | - |
| winefix | Wine stutter fix | not needed: native client | covered | - |

### Ashita v3 plugins

| Add-on | What it does | API used | Status | Missing |
| --- | --- | --- | --- | --- |
| Affinity | Sets the process CPU affinity | client.set_affinity / set_priority / process (permission process) | covered | - |
| Ashitacast | XML gear swaps around every action | filters.typed; actions.equip; event `action`; packets.on_out | covered | (packet batch order) |
| Bellhop | Batch get / put / drop / sell / buy / trade / sort | actions.move / drop / sell / buy / trade / sort; game.shop; packets.inject_out (player trade, bazaar) | covered | (packet layouts) |
| Crafty | Repeated synthesis with stop conditions | native synthesis amount extra; packets.inject_out(0x096); event `chat_message`; game.bag_size | covered | (packet layouts) |
| DashCam | Records the game to video | client.record / stop_recording | covered | - |
| Dats | Replacement DAT files | native mod manager (DAT overlays) | covered | - |
| Deeps | Damage meter with bars | events `action` / `damage`; hud.bar; the bundled /dps mod; hud.image | covered | - |
| DressMe | Fetches a profile's gear from storage | game.items; actions.move | covered | - |
| Duration | Durations of buffs cast; job point request | event `action`; packets.inject_out; game.job_points | covered | (packet layouts) |
| GearLock | No model blinking during gear swaps | packets.on_in holds the look in 0x00D / 0x051 during swaps | covered | (look / race override in render) |
| Greed | Unclear (inferred: on-screen target indicator) | draw.ring; hud; actions.target; hud.image | covered | - |
| Guildwork | Syncs character data with a site through a helper | net.request to the site (as Windower's Guildwork plugin); client.run; net.connect / listen / udp | covered | - |
| HardwareMouse | Hardware cursor | not needed natively | covered | - |
| Lootwhore | Auto lot / pass / drop / move by profile | game.treasure; actions.lot / pass / drop / move | covered | - |
| Minimap | Minimap overlay | as minimap (v4 plugin); hud.image; game.map / map_pos; hud.map | covered | - |
| MultiSend | Commands to boxed clients on this PC; follow | ipc.send; event `ipc_message`; commands.run; actions.follow | covered | - |
| No-ckback | No knockback on us | packets.on_in(0x028) edit | covered | (packet layouts) |
| Repeat | Repeats commands with random jitter | timer.after; math.random; commands.run | covered | - |
| Screenshot | Screenshots | as screenshot (v4 plugin); client.screenshot | covered | - |
| Servo | Multibox control over LAN / internet | ipc.send (this PC only); net.connect / listen / udp | covered | - |
| Shorthand | Lenient action typing, partial names | filters.typed; res.spell / ability (scan by id); game.entities | covered | - |
| StatusTimers | Status icons with timers | game.buffs (remaining); hud.icon / text | covered | - |
| vnm | Distance / direction to a VNM | packets.on_in; hud.text; draw.line | covered | - |
| WatchDog | Wide Scan tracking by mob name | game.npc_names; actions.track | covered | - |
| WatchEXP | Exp / limit point rates | event `battle_message` or filters.chat; game.stats; hud.text | covered | - |
| WindowerInput | External programs send commands, keys, text | as thirdparty; net.connect / listen / udp | covered | - |
| XIPivot | DAT and sound overlays | native mod manager (DAT and sound overlays) | covered | - |

### Ashita community plugins

| Add-on | What it does | API used | Status | Missing |
| --- | --- | --- | --- | --- |
| Ashitacast / LegacyAC | As Ashitacast (v3 plugin) | as Ashitacast (v3 plugin) | covered | (packet batch order) |
| Bellhop | As Bellhop (v3 plugin) | as Bellhop (v3 plugin) | covered | (packet layouts) |
| ChatHistoryPlus | Longer chat history | native: the session log is not capped (inferred from code) | covered | - |
| ChatLogFix | Chat buffer race fix | not needed natively | covered | - |
| Crossbar | Controller macro crossbar | hud.image / icon / on; binds.set; actions.* (keys work, the pad does not) | partly | gamepad input (deferred) |
| DatSeeker | Model ids and ROM paths of gear as worn | game.entity look; game.equipment; res.dat / dat_path; res.model_files; Entity.model / look | covered | - |
| Deeps | Damage meter | as Deeps (v3 plugin) | covered | - |
| Defrag | Combines item stacks | actions.sort | covered | - |
| DiscordRPC | Discord rich presence | none; client.run could start a bridge program; net.pipe | covered | - |
| EquipViewer | Equipment grid overlay | game.equipment; hud.icon / rect | covered | - |
| FindAll | All characters' bags cached; instant bag load after zoning | game.items; fs under shared/; packets.inject_in replaying cached bag packets | covered | (packet layouts) |
| FocuserHelper.dll | Enumerates and focuses game windows | ipc.send to each window; client.focus; `focus_change` | covered | - |
| FrameFix | 60 fps with UI and motion timing corrected | native unlocked frame rate with timing kept | covered | - |
| GBinder | Logitech G-keys run commands | binds.set (G-keys mapped to keys by the vendor tool); devices.list / open / write (raw HID reports) | covered | - |
| gdifonttexture.dll | GDI+ text drawn to textures for add-ons | hud.text | covered | (font families) |
| Guildwork | Launches a site sync helper | net.request to the site (as Windower's Guildwork plugin); client.run; net.connect / listen / udp | covered | - |
| Lootwhore | As Lootwhore (v3 plugin) | as Lootwhore (v3 plugin) | covered | - |
| Multisend | Command broadcast, groups, follow | ipc.send (to a character or all); commands.run; actions.follow | covered | - |
| Nameplate | Nameplate aspect fix | not needed: our renderer | covered | - |
| Novo | Navmeshes: build, draw, test; collision export | game.collision / collision_mesh / raycast / floor; draw.line; draw.text / draw.image; fs | covered | - |
| Occlude3d | Depth-tested world markers, lines, text | draw.line / ring / box (depth-tested); draw.to_screen; mods.call; draw.text / draw.image | covered | - |
| Packer | Moves gear between bags for a job's sets | actions.move; mods.call / mods.emit with the gear mod | covered | - |
| PacketFlow | Changes the client's packet send timing | client.set_option('send_interval', ms) | covered | - |
| portfix | Fixes port-era effects drawn wrong | not needed: our renderer | covered | - |
| Sequencer | Keeps sequence ids on re-sent packets | packets.on_out (header bytes editable) | covered | - |
| Servo | Multibox control over the network | as Servo (v3 plugin); net.connect / listen / udp | covered | - |
| Shorthand | As Shorthand (v3 plugin); fuzzy target names | as Shorthand (v3 plugin) | covered | - |
| Snap | Snaps a boxed window to a saved place / size | client.window / set_window; storage; client.set_window | covered | - |
| Stun | Reactive auto-stun from rules | event `action`; actions.cast (automation) | covered | - |
| Stylist | Any player's look by command; anti-blink | packets.on_in(0x00D, 0x051) edit | covered | (look / race override in render) |
| TrueFont | Game text in chosen TrueType fonts | native modern UI fonts; client.set_option('ttf_text', true) | covered | (font families) |
| TrueFPS | High frame rates; faster cutscenes; stall recovery | native unlocked frame rate; client.set_option('cutscene_speed', n) | covered | - |
| Uberwarp | NPC warp systems by command | actions.talk / choose; packets.inject_out(0x05B) | covered | (packet layouts) |
| VNM | Line to the last known VNM position | draw.line | covered | - |
| WeatherFX | Sets the zone weather shown | packets.inject_in(0x057) (until the server's next weather) | covered | (packet layouts) |
| XIPivot | DAT and sound overlays, manager UI | native mod manager and Mods page | covered | - |
| XIVUi | Party list with buffs | game.party buffs; hud.text / bar / icon; ui.native.hide('party') | covered | - |

### Ashita v4 community add-ons

| Add-on | What it does | API used | Status | Missing |
| --- | --- | --- | --- | --- |
| addon-emoji | Inline emoji icons in chat | filters.chat; chat.icon | covered | - |
| addonmgr | Loads, unloads and reloads add-ons from a panel | commands.run('/lua ...'); mods.list; ui.window (input, tabs); mods.installed | covered | - |
| ahpro | The auction house in one window over the native one | input.press; game.items; res.item; hud.icon; ui.window (input, tabs, section, choice, tips, canvas); ui.prompt / menu; ui.native.list (the auction lists and menus); ui.native.highlighted | covered | - |
| ASCII-Joy | MUD-style text interface over the game | game.*; packets.on_in / on_out / inject_out; filters.chat; hud.text / rect; res.mob_skill; Entity.target_index; game.cast | covered | (font families) |
| ashita-hush | Hides yells, teleport requests and other spam | filters.chat; ui.window (options); storage | covered | - |
| Ashita4-HitboxDisplay | 3D hitboxes and ranges | game.entities; draw.ring / box / line; ui.window (choice, colour); Entity.hitbox | covered | - |
| Ashita4-Soul | Soul plate and reflector viewer | game.items (extdata bytes); res.item | covered | - |
| AshitaFishaid | Bigger fishing messages | packets.on_in(0x037, 0x00A); filters.chat; hud.text | covered | (font families; packet layouts) |
| Auctioneer | Auction house by command, price lookups | packets.inject_out(0x04E) / inject_in(0x04B, 0x04C); net.request; net.open_url; ui.window (input, columns, tabs, choice, tips, canvas); ui.prompt / menu | covered | (packet layouts) |
| Audible | Sound cues on events | packets.on_in; audio.play | covered | (packet layouts) |
| autoassist | Assists a player, faces the mob, walk toggle | game.party / entity; actions.assist / attack / turn / walk; game.info().follow | covered | - |
| autochecker | Checks each new target | event `target_change`; commands.run('/check') | covered | - |
| autoenterkey | Presses Enter on a timer | input.press('enter'); timer.every | covered | - |
| autofps | Moves the fps divisor to a stable rate | native frame limit (default 300); event `frame` dt | covered | (engine settings from Lua) |
| autonin | Ninja housekeeping (shadows, tools) | game.buffs / items / recasts; actions.cast / cancel; packets.on_out; input.on_key | covered | - |
| autora | Repeats ranged attacks | event `action`; actions.ranged; binds.set; storage | covered | - |
| autotrans | [[ ]] in typed commands to auto-translate phrases | filters.typed; res.autotranslate; text.autotranslate / phrase | covered | - |
| BGMBard | Day / night music in any zone | packets.on_in(0x00A music ids); ui.window (input, choice, section) | covered | (packet layouts) |
| bludex | Blue magic codex and set planner | game.job_data / spells; res.spell; actions.blue_set / blue_remove; hud.image / on; ui.window (canvas) | covered | (packet layouts) |
| bluspells | Builds blue magic sets visually | game.job_data; res.spell; actions.blue_set; ui.window | covered | - |
| boussole | Replacement map with markers and tracked mobs | game.entities / party; packets; draw.to_screen; hud.*; ui.window (input, columns, tabs, section, choice, colour, tips, canvas); fs; ui.prompt / menu; client.paste; game.map / map_pos; hud.map; canvas drag events | covered | - |
| boxhelper | Hints for brown treasure casket numbers | filters.chat; packets.on_in(0x034); ui.window (columns) | covered | (packet layouts) |
| ButterFeet | Removes the job-ability movement wait (ja0wait patch) | client.set_option('move_lock', false) | covered | - |
| calculator | In-game calculator | ui.window (buttons, text) | covered | - |
| captain | Packet capture and analysis suite | packets.on_in / on_out; fs; game.*; ui.window (input, tabs, tips); binds.set; res.mob_skill; net.connect / listen / udp | covered | (SQLite; packet layouts) |
| castdelay | Holds casts, items and ranged until standing still | packets.on_out (block, re-inject); game.player (pos) | covered | (packet layouts) |
| cBind | Binds controller buttons to commands | binds.set; ui.window (input, columns, choice, tips); ui.prompt / menu | missing | gamepad input (deferred) |
| chains | Skillchain options for the target | event `action`; game.target / job_data; res.ability; hud.text | covered | - |
| chatty | A nicer chat window | filters.chat; ui.window (list, input, tabs, tips); commands.run; fs | covered | (inline colours in window text; font families) |
| Clammy | Clamming bucket weight, contents and value | filters.chat; game.items / time; ui.window (tabs, tips); audio.play; fs | covered | - |
| cleanup | Blacklist and log filter | packets.on_in(0x017, block); filters.chat; ui.window (input, tabs); fs | covered | - |
| clearpartybars | Cleaner party bars | game.party / target; hud.bar / text; ui.native.hide('party') | covered | (font families) |
| colure | Geomancer spell menu | game.*; actions.cast; ui.window; ui.prompt / menu | covered | - |
| ctimers | Simple custom timers | commands.register; hud.text; timer | covered | (font families) |
| cudgel | Equips the best ready warp cudgel | game.items (extdata bytes); actions.equip | covered | - |
| cureplease-addon-ashitav4 | UDP bridge to the external CurePlease bot | packets.on_in; commands.run; net.connect / listen / udp | covered | - |
| customcolors | Colours chat lines for custom content | filters.chat (mode) | covered | - |
| customtarget | Custom target cursor | game.target; draw.to_screen; hud.image | covered | - |
| database | Tracks needed items, gil and points | game.items / gil; packets.inject_out(0x10F, 0x115) / on_in(0x113, 0x118); ui.window (columns, tabs, choice, tips) | covered | (packet layouts) |
| deeps | Damage meter | events `action` / `damage`; game.party / entity; res.*; hud.rect / text / on; input.on_mouse; mods.emit | covered | - |
| dlac | Gear sets, triggers, live stats; equips by itself | packets.on_out (block) and inject_out (equips, then the action); actions.equip; game.*; res.*; binds.set; input; ui.window (input, tabs, section, choice, tips, canvas); hud.*; mods.emit; fs; load; res.mob_skill; ui.prompt / menu | covered | (packet batch order; packet layouts) |
| dxui | Light party and target HUD | game.party / target / entity; packets.on_in; hud.* | covered | (font families) |
| fastcs | Uncaps the frame rate in cutscenes to speed them | client.set_option('cutscene_speed', n) while `game.event()` shows | covered | - |
| ffxi-ashita-minimapcontrol | Hides the minimap by zone and state | event `zone_change`; game.info; ui.native.hide('minimap') / menu_open; fs | covered | - |
| ffxi-ashita-upcast | Casts the highest known tier of a spell | game.spells / recasts / player; res.spell; actions.cast | covered | - |
| ffxi-balloon-ashitav4 | NPC dialogue in speech balloons with portraits | filters.chat; actions.advance; ui.native.interface; game.event; hud.image / text; input.on_key / on_mouse | covered | (gamepad input, deferred; font families) |
| ffxi-zonename | Zone and region name while zoning | event `zone_change`; res.zone; hud.text; timer | covered | (font families) |
| FFXIQuestList | Quests ready to take | packets.on_in(0x056); bundled data; ui.window (tabs, tips); net.open_url | covered | (packet layouts) |
| find | Finds items across bags | game.items; res.item; commands.register | covered | - |
| fishaid | Bigger fishing messages | packets.on_in; filters.chat; hud.text | covered | (font families; packet layouts) |
| Focuser | Switches focus between game windows | ipc.send; client.focus; `focus_change`; ui.window (input, tips) | covered | - |
| gambler | Corsair auto-roll by rules | event `action`; game.buffs / recasts; actions.ability; ui.window (input, columns, tabs) | covered | - |
| GlamourUI | Themed party list, target bar, chat and map | game.party / target / entity / buffs / recasts; packets.on_in / on_out; hud.*; ui.window (tabs, section, choice, colour, tips, canvas); ui.native.hide / config; input.held; res.mob_skill; game.cast; ui.prompt / menu; game.map / map_pos; hud.map | covered | (gamepad input, deferred) |
| GMTools | GM command helper | commands.run; game.*; ui.window (input, columns, tabs, choice, tips); client.copy; client.paste; ui.prompt / menu | covered | - |
| HGather | Gathering tracker | filters.chat; packets.on_out; input.on_key / on_mouse; ui.window (input, tabs, tips) | covered | - |
| HideActions | Hides chosen spells, abilities and mounts from lists | packets.on_in(0x0AA, 0x0AC, 0x0AE); res.spell / ability; res.mount | covered | (packet layouts) |
| HitPoints | Target and engaged-enemy bars | game.target / entities; hud.bar / text / image; Entity.target_index; ui.window (tabs, choice, tips, canvas) | covered | (font families) |
| HorizonScout | Nearby alerts, aggro warnings, compass radar | game.entities / target / info; packets.on_in; hud.*; audio.play; actions.target; client.paste; ui.window (input, tabs, section, tips, canvas) | covered | (font families) |
| hticks | Time to the next resting tick | event `status_change`; timer; hud.text; input.on_mouse; ui.window (colour, tips) | covered | (font families) |
| Hush | Hides yells, teleport requests and other spam | filters.chat; game.party; ui.window (options); storage | covered | - |
| HXIChecklist | Quest and mission checklist | packets.on_in(0x055, 0x056); game.items / key_items; ui.window (input, columns, tabs, choice, tips); net.open_url | covered | (packet layouts) |
| HXIPresence | Discord rich presence | game.player / zone; ui.window; client.run could start a bridge program; net.pipe | covered | - |
| HXIUIBegone | Hides chosen parts of the native UI | ui.native.hide; ui.window (tips); ui.prompt / menu | covered | - |
| imrecast | Recast timers | game.recasts; res.spell / ability; hud.text | covered | - |
| invite | Invites listed players automatically | commands.register; timer.every; actions.invite | covered | - |
| j-roller | Corsair auto-roller | event `action`; game.buffs / recasts; actions.ability; ui.window (section, choice, tips) | covered | - |
| loggers | Raw packet log to a file | packets.on_in / on_out (all); fs.append | covered | - |
| LootScope | Loot statistics and Treasure Hunter monitor | packets.on_in / on_out; filters.chat; game.*; ui.window (input, columns, tabs, section, choice, tips, canvas); fs; ui.prompt / menu | covered | (packet layouts) |
| LuAshitacast | Gear swaps from Lua profiles | packets.on_out (block) and inject_out (0x050 / 0x051 / 0x053, then the action); actions.equip; game.*; res.*; fs; load; mods.emit; ui.window (tabs, choice, tips); hud.text; client.run | covered | (font families; packet batch order; packet layouts) |
| luopantime | Luopan countdown | game.entities; hud.text; timer | covered | (font families) |
| macromaster | Switches macro book and set by command or job | commands.run('/macro book', '/macro set'); event `job_change`; storage | covered | - |
| magiccircle | Textured magic circle in the world (Occlude3d) | draw.ring; ui.window (colour, tips); draw.text / draw.image | covered | - |
| maneuver | PUP maneuver tracker | events `buff_gain` / `buff_lose`; filters.chat; game.buffs; hud.image / bar / text | covered | - |
| MemScope | Memory monitor for add-ons | mods.list; ui.window (input, columns, choice, tips, canvas); ui.prompt / menu | covered | - |
| minimap-helper | Minimap zoom per zone | event `zone_change`; packets.on_in(0x00A); client.set_option(minimap_span) | covered | - |
| mobdb | Monster information from bundled data | game.target; require (data); hud.*; ui.window (input, tips); mods.emit | covered | - |
| mountme | Mounts by command | commands.register; game.key_items; res.key_item; actions.mount | covered | - |
| muffins | Sortie gallimaufry count | filters.chat; hud.text | covered | (font families) |
| nextauto | Swing timer as an arc or a bar | event `action`; game.equipment; res.item (delay); hud.bar; ui.window (canvas, colour) | covered | (arc primitive) |
| ninhelper | Ninja helper (shadows, tools) | game.buffs / items / recasts; actions.cast; ui.window (tips) | covered | - |
| nocombat | Keeps the zone music in battle | packets.on_in(0x00A) | covered | (packet layouts) |
| nolock | Removes the engage animation lock | client.set_option('move_lock', false) | covered | - |
| nomnom | Eats food when it wears off | game.buffs / items; actions.use_item; ui.window (choice, tips) | covered | - |
| nomorecrap | Queues usable items and command blocks | commands.run; actions.use_item; timer; game.items; ui.window (input, columns, tabs, tips); ui.prompt / menu | covered | - |
| NoMount | Blocks the mount music | packets.on_in(0x00A, 0x05F) | covered | (packet layouts) |
| omen | Omen objectives tracker | packets.on_in(0x00A, 0x027, 0x02A, 0x036); hud.text | covered | (dialogue text by id; packet layouts) |
| openmenus | Opens the magic and ability menus by command | actions.open_menu | covered | - |
| Oseem | Automatic augmenting with Oseem | packets.on_in(0x032, 0x034, 0x05C) / inject_out(0x05B); game.items; ui.window (tabs, choice) | covered | (packet layouts) |
| partybuffs | Party buffs beside the native party list | native party status icons; game.party (buffs); hud.icon | covered | (native window bounds; font families) |
| PetMe | Detailed pet information | game.info().pet_index; game.entity; packets.on_in(0x068); game.recasts; ui.native.windows; hud.*; ui.window (colour, tips) | covered | (packet layouts) |
| PetsReborn | Pet HUD for every pet job | game.entity; game.info().pet_index; packets.on_in(0x068); hud.*; ui.window (input, tabs, section, choice, colour, tips, canvas); res.mob_skill | covered | (font families; packet layouts) |
| phantomgem | Buys phantom gems from the NPC | actions.talk / choose; game.event; packets.on_in(0x034) | covered | (packet layouts) |
| points | Experience, limit, capacity and currency tracking | packets.on_in(0x02D, 0x061, 0x063, 0x110, 0x113); game.stats / job_points; filters.chat; hud.*; ui.window (input, columns, tabs, section, choice, colour, tips) | covered | (font families; packet layouts) |
| popper | Trades pop items to NPCs and ??? | game.items / target; actions.trade; fs | covered | - |
| Porter | Porter moogle storage and retrieval | packets.inject_out(0x029, 0x036, 0x05B); game.items; actions.trade; ui.window (input, tips); mods.emit | covered | (packet layouts) |
| pos | Copies the position to the clipboard | game.player; client.copy | covered | - |
| prism | Skill progress overlay | game.skills; hud.*; ui.window (canvas, colour, tips) | covered | - |
| puphelper | Keeps PUP maneuvers up | game.buffs / recasts; actions.ability; ui.window (choice, tips) | covered | - |
| puphelper (Pup-Helper repo) | Keeps PUP maneuvers up | game.buffs / recasts; actions.ability; ui.window (choice, tips) | covered | - |
| pupsets | Automaton attachment sets by command | game.job_data; actions.attach / detach; fs | covered | - |
| rcheck | Party ready check | event `chat_message`; game.party; commands.run; timer | covered | - |
| relicge | Dynamis relic farming stats | filters.chat; ui.window; fs | covered | - |
| repl | Lua REPL window | load; ui.window (input) | covered | (multi-line text input) |
| RollTracker | Tidier Corsair roll lines | packets.on_in(0x028); filters.chat | covered | (packet layouts) |
| runehelper | Keeps RUN runes up | game.buffs / recasts; actions.ability; ui.window (choice, tips) | covered | - |
| ScentHound | Spawn tracking with alerts and markers | packets.on_in(0x00E, 0x0F5) / inject_out(0x016); draw.*; hud.*; audio.play; ui.window (input, tabs, choice, colour, tips) | covered | (packet layouts) |
| segments | Odyssey segment count | filters.chat; hud.text | covered | (font families) |
| Segs | Odyssey segment tracker and report | filters.chat; packets.on_in(0x00A); hud.text; timer | covered | (packet layouts) |
| sellit | Sells to the open NPC shop by item name | game.shop / items; actions.sell | covered | - |
| shades | Dims a too-bright screen | hud.rect; ui.window (colour) | covered | - |
| SimpleLog | Combat log rewriter | events `action` / `battle_message`; packets; filters.chat; res.*; ui.window (tabs, section, choice, tips); fs; res.mob_skill; res.message; ui.prompt / menu | covered | (res log-form status names; packet layouts) |
| skillchain | Skillchain tracker | event `action`; game.entity; res.ability; hud.text; timer; res.mob_skill | covered | - |
| skilluptracker | Decimal skill and cap in skill-up lines | packets.on_in(0x029); filters.chat; game.skills; ui.window | covered | (packet layouts) |
| skillwatch | Box of the abilities and spells mobs start | filters.chat; game.entity; hud.text; ui.window (input, tabs, choice, canvas); audio.play; storage | covered | (font families) |
| songcast | Ranges of friendly bard songs | event `action`; game.party / entities; draw.ring | covered | - |
| SpellSelect | Casts by element and tier | game.spells / recasts; res.spell; actions.cast | covered | - |
| statustimers | Status icon row with timers, sorting, click to cancel | game.buffs; game.party (buffs); hud.icon / text / on; actions.cancel; ui.native.hide('buffs'); ui.window (columns, choice, input, colour, tips) | covered | - |
| stepdown | Steps a spell down a tier while it recasts | game.recasts / player; res.spell; actions.cast | covered | - |
| strats | Scholar stratagem charges | game.recasts / player; hud.image / text | covered | - |
| targetinfo | Target information overlay | game.target; hud.text | covered | - |
| tCrossBar | Controller crossbar of actions | game.*; actions.*; hud.image / text / on; packets; ui.window (input, tabs, choice, tips) | partly | gamepad input (deferred); (font families) |
| tHotBar | Clickable keyboard hotbar | binds.set; hud.image / icon / text / on; game.recasts; actions.*; packets; ui.window (input, tabs, choice, tips) | covered | (font families) |
| thtracker | Treasure Hunter level per mob | filters.chat; game.entity; hud.text | covered | (font families) |
| timers | Durations of our spells and abilities, buffs on others | events `action` / `battle_message`; game.buffs / recasts; packets.on_in / on_out; hud.bar / text; input.on_mouse; ui.window (input, tabs, colour, tips) | covered | (font families; packet layouts) |
| TitleCheck | Prints titles not yet earned | packets.on_in(0x033); res.title; game.stats | covered | (packet layouts) |
| toastynotif | Toast notifications with sounds | filters.chat; packets.on_in(0x0D2); hud.image / text; audio.play; fs.list; ui.window (columns, section, choice) | covered | (packet layouts) |
| translataru | [[ ]] in typed text to auto-translate phrases | filters.typed; res.autotranslate; text.autotranslate / phrase | covered | - |
| trials | Trial weapon weapon-skill points | event `action`; game.items (extdata bytes); ui.window (columns) | covered | - |
| trigger | Controller shoulder buttons to commands | none | missing | gamepad input (deferred) |
| trustme | Searchable trust list with wiki links | game.spells; res.spell; actions.cast; net.request; net.open_url; ui.window (input, columns, tabs, section, choice, colour, tips); ui.prompt / menu | covered | - |
| tTimers | Buff and debuff timers | events `action` / `battle_message`; game.buffs; hud.*; input.on_mouse; res.mob_skill; ui.window (tabs, choice, tips) | covered | (font families) |
| Warn | Encounter helper, debuff and crowd-control tracking | events `action` / `battle_message`; game.*; hud.*; audio.play; net.request; fs; res.mob_skill; ui.window (input, section, choice, canvas) | covered | (gamepad input, deferred; font families) |
| watcher | Watches for named mobs nearby | game.entities; hud.text; ui.window (input) | covered | - |
| weather | Small current-weather window | game.weather; event `weather_change`; hud.text; hud.on | covered | - |
| weatherhelper | Party call when a chosen weather starts or ends | event `weather_change`; commands.run('/p ...') | covered | - |
| wheel | Gavlan wheel and deal helper | packets.on_in(0x01E..0x020); game.items / recasts; hud.*; ui.window (canvas) | covered | (packet layouts) |
| whereisdi | Domain Invasion location from a web API | net.request; chat.add | covered | - |
| wiki | Opens bg-wiki for the highlighted item, target or text | game.target; net.open_url; ui.native.highlighted | covered | - |
| wizardui | Clickable black mage buttons | game.*; hud.image / icon / on; actions.cast; packets.on_in; ui.window (input, section, canvas) | covered | - |
| xicamera | Camera distance, battle distance, pan speeds | render.set_camera {distance, battle_distance, speed}; ui.window (section, tips) | covered | (camera rotation and pitch limits) |
| xichats | Chat channels in a window | packets.on_in(0x017) / on_out(0x0B5); event `chat_message`; ui.window (input); commands.run; fs | covered | (inline colours in window text) |
| xitools | UI toolkit: target, recast, crafting, fishing and more | game.*; packets; hud.*; ui.window (input, columns, tabs, section, colour, tips, canvas); res.day / moon; Entity.target_index; game.cast; ui.prompt / menu | covered | - |
| XIUI | Full HUD suite: bars, party, target, hotbar, more | game.*; actions.lot / pass; packets; hud.*; ui.window (input, columns, section, choice, colour, tips, canvas); ui.native.windows / hide / interface_hidden; audio.play; res.mob_skill; Entity.target_index; game.cast; ui.prompt / menu; game.macros; actions.macro / macro_set / macro_edit; ui.native.hide; res.mount | covered | (gamepad input, deferred; font families) |
| yunaui | Full HUD suite with chat and damage meter | game.*; packets; hud.*; ui.window (input, colour, tips, canvas); input.*; audio.play; res.mob_skill; Entity.target_index; game.cast; client.paste; ui.prompt / menu; game.macros; actions.macro / macro_set / macro_edit; game.map / map_pos; hud.map | covered | (gamepad input, deferred) |
| zenihelper | ZNM pops, trophies and key items | game.items / key_items; packets.inject_out(0x10F) / on_in(0x113); ui.window (tips) | covered | (packet layouts) |
| ZoneLines | Zone line boundaries marked on the ground | game.zone / player; draw.line / box / ring; draw.to_screen; hud.text; require (bundled data); ui.window (columns, section, choice, colour, tips) | covered | (font families) |
| zonename | Zone name while zoning | event `zone_change`; res.zone; hud.text | covered | (font families) |

### Ashita v3 community add-ons

| Add-on | What it does | API used | Status | Missing |
| --- | --- | --- | --- | --- |
| Auctioneer | Auction house by command through packets | packets.inject_out(0x04E) / inject_in(0x04B, 0x04C); game.items; hud.text | covered | (font families; packet layouts) |
| AutoBurst | Magic bursts on skillchains | event `action`; actions.cast; game.player | covered | - |
| boxdestroyer | Solves treasure casket guesses | packets.on_in / on_out(0x02A, 0x034, 0x05B); chat.add | covered | (packet layouts) |
| debuff-tracker | Debuffs on mobs | events `action` / `battle_message`; hud.text | covered | (font families) |
| Fast Cutscene | Uncaps the frame rate in cutscenes to speed them | client.set_option('cutscene_speed', n) while `game.event()` shows | covered | - |
| Gobbiebox | Trades keys to a Gobbiebox | game.items; actions.trade; filters.chat; timer | covered | - |
| Menus | Opens game menus by command | packets.inject_in(0x02E, 0x04C) / inject_out(0x04D) | covered | (packet layouts) |
| NPCit | Sells items to NPC shops by command | game.shop / items; actions.sell | covered | - |
| PartyBuffs | Party buffs beside the party list | native party status icons; game.party; hud.icon; packets.on_in(0x076) | covered | (native window bounds; font families) |
| pupsets_ashita | Saves and loads automaton attachment sets | game.job_data; actions.attach / detach; fs | covered | - |
| React | Runs commands in reaction to mob actions | event `action`; commands.run; actions.turn; res.*; res.mob_skill | covered | - |
| RollTracker | Tidier Corsair roll lines | packets.on_in(0x028); filters.chat | covered | (packet layouts) |
| ScanZone | Wide Scan tracker for named mobs | packets.on_in(0x00E) / inject_out(0x016); actions.track; ui.window (list, input, section) | covered | (packet layouts) |
| SendTarget | Sends the target to boxed characters | ipc.send; actions.target | covered | - |
| seven | Multibox control network | ipc.send; event `ipc_message`; packets; commands.run | covered | - |
| Skillchains | Skillchain and burst window | event `action`; hud.text; timer; res.ability | covered | (font families) |
| smnbar | Summon and blood pact bar | hud.icon / image / on; actions.cast / ability; input.on_key | covered | - |
| THTracker | Treasure Hunter procs in a window | filters.chat; ui.window | covered | - |
| Trade | Trades to players and NPCs by command | packets.inject_out (trade packets); actions.trade | covered | (packet layouts) |
| TurnAround | Faces away from or toward the target | actions.turn; game.target | covered | - |
| UnityNMFighter | Sparks, Unity NM and coffer automation | actions.*; game.*; filters.chat; commands.run; binds.set | covered | - |
| xicamera | Camera distance, battle distance, pan speeds | render.set_camera {distance, battle_distance, speed} | covered | (camera rotation and pitch limits) |

### Ashita add-ons rated from descriptions

No source was scanned for these; they are rated from their one-line descriptions.

| Add-on | What it does | API used | Status | Missing |
| --- | --- | --- | --- | --- |
| Att | Linkshell attendance to a file | game.party / entities; fs.write | covered | - |
| Attendance | Linkshell attendance to a file | game.party / entities; fs.write | covered | - |
| BigMode | Changes the player's model size | render.entity (scale) | covered | - |
| Cosplay | Copies a target's appearance | game.entity().look; packets.on_in / inject_in (the look in 0x051 / 0x00D) | covered | (look / race override in render) |
| DigDig | Chocobo digging helper | filters.chat; game.items; hud.text | covered | - |
| DKPBids | Linkshell DKP bidding, written to files | event `chat_message`; fs.write / append | covered | - |
| Emotes | Emote grid | ui.window (buttons); commands.run('/emote') | covered | - |
| ENMs | ENM key item timers | game.key_items; storage; timer; ui.window | covered | - |
| EventTracker | Linkshell event log to a file | event `chat_message`; game.party; fs.append | covered | - |
| GearFinder | Finds gear across characters | game.items; storage.account or fs `shared/`; ui.window | covered | - |
| Giltracker | Gil tracking | game.gil; hud.text | covered | - |
| InvTracker | Inventory tracker | game.items / bag_size; hud.text | covered | - |
| KParser | External parser reading the log | filters.chat; fs.append (a log file for the tool) | covered | - |
| Lootz | Loot tracker | packets.on_in(0x0D2, 0x0D3); filters.chat; fs | covered | (packet layouts) |
| lschat | Linkshell chat window | event `chat_message`; ui.window (list, input); commands.run | covered | (inline colours in window text) |
| Meteorologist | Weather information | game.weather / time; hud.text | covered | - |
| Metrics | Battle parser | events `action` / `battle_message` / `damage`; ui.window; fs.write | covered | - |
| NinjaTool | Ninja tool counts and helper | game.items / buffs; hud.text | covered | - |
| Packrat | On-screen item list | game.items; hud.text | covered | - |
| Parse | Battle parser with XML export | events `action` / `battle_message` / `damage`; fs.write | covered | - |
| PlayerInfo | Player information display | game.player / stats; hud.text | covered | - |
| PriceCheck | NPC base price of the item under the cursor | res.item; bundled prices; ui.native.highlighted | covered | - |
| Rest | Resting tick timer | event `status_change` / `mp_change`; hud.text; timer | covered | - |
| RSVP | Reminders at set times | timer; chat.add; storage; audio.play | covered | - |
| TargetLines | Lines from entities to their targets | draw.line; event `action` (recent targets); Entity.target_index | covered | - |
| Ticker | Resting tick timer | event `status_change` / `hp_change`; hud.text; timer | covered | - |
| XivParty | Party list (Windower port) | game.party; hud.*; ui.native.hide('party') | covered | - |
| ZoneTimer | Time spent in the zone | event `zone_change`; clock; hud.text | covered | - |
