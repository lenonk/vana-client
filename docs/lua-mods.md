# Lua mods: design

The add-on surveys behind it are in MODLOG "Lua mods: the add-on surveys".

## State

Built (client/lua/, modern/modwin.rs), each checked live:
- **Runtime:**
  - a VM per mod (Lua 5.4, mlua 0.12): 38 to 64 KB each, measured on F12
  - restricted libraries, text-only `load`, a 64 MB memory limit
  - a 100 ms budget per call (a runaway loop is stopped)
  - coroutine handlers (`sleep`, `sleep_frames`, `wait_until`)
  - errors reported once with file:line; 20 in a minute suspend the mod
  - `require` from the mod's folder
  - `/lua list | load | unload | reload | run | set | settings | grant | revoke`; `/lua reload` alone reloads every
    mod, and a mod that failed to load is loaded; `/lua watch <mod>` reloads a mod whenever its files are saved (kept
    in mod-data/watch.json), so a mod is written without restarting the game
  - LuaLS stubs written into the mods folder at login (`.library/vana.lua`, `.luarc.json`): open the mods folder in
    an editor with the Lua language server for completion and checks
  - F12 lines per mod (time, peak, slow calls, memory, errors)
  - LuaLS stubs (`vana-client lua-stubs`) from docs.rs, kept equal to the bindings by a test
- **Events:**
  - lifecycle: `load`, `unload`, `login`, `logout`, `zone_change`, `frame`
  - Windower's change events from a per-frame diff: vitals, status, job, level, buffs, target, time, day, moon,
    weather, party, items
  - from packets: `chat_message`, `action` (0x028 decoded), `battle_message`
  - `settings_changed`
- **State:**
  - `game.player`, `entity` / `entities` / `target` (ourselves included; race, model, look, party flags), `party`,
    `items`, `bag_size`, `equipment`, `buffs`, `recasts`, `time`, `weather`, `key_items`
  - `game.entity_by_tag` (t, st, bt, pet, ht, scan, p0..p5, a10..a25, names), `skills`, `jobs` (levels, master
    levels), `stats` (base and bonus, attack, defense, resists, item level, rank, nation, experience, merit and limit
    points), `merits`, `job_points`, `gil`, `spells` (learned), `abilities` (usable), `job_data` (BLU spells, PUP head,
    frame, attachments, raw), `info` (server, Mog House, chat / menu open, combat, lock, autorun, follow, pet, map
    grid), `camera`, `npc_names` (the zone list); checked live
  - `res.item`, `spell`, `ability`, `status`, `zone`, `title`, `key_item`, `job`
- **Chat and commands:**
  - `chat.add` (mode colours)
  - `filters.chat` (keep, drop or rewrite every line) and `filters.typed` (typed lines before the parser)
  - `commands.register` (`//name`, `/name`), `commands.run`
- **Settings and storage:**
  - declared settings (bool, enum, int, number) on an Extras page per mod, in both UIs; per-scope files
  - `storage.account` / `storage.character`
- **Windows:**
  - `ui.window` / `update` / `open` / `show` / `close` / `on`
  - elements: text, buttons and options, slider, scrolling list, gauge, separator
  - keys and mouse in both UIs: the classic UI draws Config pages, the modern UI kit panels with saved places
  - HUD windows (no focus, frameless, placed)
- **Permissions:** each one a mod asks for is a row on its Extras page (ON / OFF, with what it allows on the help
  line), or `/lua grant` / `revoke`; kept in mod-data/grants.json. A mod loaded without them says where to allow them.
- **Packets** (`packets.read` / `packets.write`):
  - `packets.on_in` / `on_out` (one id or all) see each packet before the client reads it or after it built it, as
    a chain: original and current bytes, blocked, injected; a handler returns new bytes, `false` to block, or nothing
  - `inject_in` (decoded as if the server sent it) and `inject_out`, both through the hooks; `build`; `last`
  - checked live: counts by id, an injected tell, a say rewritten into a GM command the server answered, across zones
- **Actions** (`actions`: only while handling the player's input, a typed command or a window event; `automation`:
  any time; a coroutine keeps how it began across sleeps; `commands.run` of a game command is an action too):
  - `cast`, `ability` / `weaponskill`, `ranged`, `use_item`, `use_slot`, `attack`, `disengage`, `assist`, `target`,
    `lock`, `heal`, each through the menus' and text commands' own code (checks, refusal lines, packets)
  - `equip` (one piece or a set table, by id or bag slot; worn pieces stay), `unequip`, `equip_set`, `move`, `sort`,
    `drop`, `lot`, `pass`, `cancel`, `choose` (an event menu option), `talk`, `scan`, `track`, `untrack`
  - movement on autorun: `run`, `run_to`, `follow`, `walk`, `stop`, `turn` (headings as entity.heading)
  - `game.treasure`, `game.event` (the menu showing) to act on
  - checked live: the gate (a timer refused under `actions`), a cast refused with the menu's line, equip by id and
    off, run_to and turn in West Ronfaure
- **Input** (`input`, `binds`):
  - `input.on_key` / `on_mouse` see every key edge and mouse event before egui and the game, as a chain across
    mods; returning true takes it (needs `input.capture`; a taken press takes its repeats and release). They are the
    player's input, so `actions` may act there.
  - `input.held`, `modifiers`, `mouse` (window pixels), `typing`; `input.press` ("ctrl+f1") and `send` (one edge),
    which reach the key functions and the UI as the keyboard's do (`input.capture`)
  - `binds.set` (exact modifiers, Windower's `^!~@` too) to a line run as typed or a function, on press or release,
    optionally while typing; the key is taken. `binds.unset`, `binds.list`
  - `chat.line` and `chat.set_line` (open the chat line with text, or close it)
  - test drives' keys and mouse go through the same filters and binds
  - checked live: a key taken (Space left the chat line shut), F9 taken, binds by function and by line, a click's
    place, the chat line opened by a mod and sent by its own Enter
- **Runtime helpers:**
  - `os.time`, `os.date`, `os.clock`, `os.difftime` (nothing else of `os`)
  - `fs.read` / `write` / `append` / `exists` / `list` / `mkdir` / `remove` / `rename` in the mod's data folder
    (mod-data/<id>/files), reads falling back to the mod's own files; `shared/...` with `files.shared`; no `..`
  - `json.encode` / `decode`
  - `mods.list`, `mods.provide` / `mods.call` (a function in another mod's VM, values copied), `mods.emit` (an event
    to every other mod)
  - `text.from_shift_jis` / `to_shift_jis` / `plain` (auto-translate phrases as words); `client.copy`
  - checked live: a call returning two values, an emitted table, files written, appended, listed and read back, the
    mod's own file read, `..` and `shared/` refused, JSON both ways, the os subset, Shift-JIS round trip
- **Bundled mods:** `/dps` (the first Extra ported).
- **Mods folder:** `mods/<id>/` (enabled through config `mods`); a test run uses `mods` beside its FFXI_CONFIG copy.

Next (docs/lua-mods.md 6): packet layouts and decoded views, batch order (the rest of
phase 4); the rest of phase 5 (trade, shops, BLU / PUP sets as actions; `ui.native`; `render`); the other Extras as
bundled mods; then world-space drawing, audio, ipc / net, and the coverage table.

## 1. The bar

The user's requirements:

- Mods can read nearly all of the client's state and change whatever is reasonable.
- Mod windows look built in. They use our widget toolkit and follow the active theme: Classic windows under the
  classic UI, the modern kit under the modern UI. No ImGui.
- **Every existing Windower and Ashita add-on and plugin must be re-implementable on this API.** Running them
  unmodified does not matter.

What that covers (counted from source, see the surveys):

| Ecosystem | Surveyed |
| --- | --- |
| Windower 4 | 127 official add-ons (115k lines), 42 shared libs, 55 widely used community add-ons, 17 current and 25 deprecated plugins, Windower's own hook features |
| Ashita | 81 official v4 add-ons, 58 v3 add-ons, 191 community add-ons (228 v4 in all), 7 + 27 + 36 plugins |

The heaviest uses: commands (Windower 102 add-ons, Ashita 167), packet reads (58 / 106), chat output (86 / 150 via
the chat lib), settings (77 / 94), per-frame drawing (42 text objects / 141 draw every frame), FFXiMain memory reads
and patches (Ashita: 86 read, 37 patch, 16 call game functions). We have no FFXiMain, so **every memory trick becomes a
named API call**. Ashita survey section 9 and Windower survey section 14 are the checklists.

## 2. How other games do it, and what we take

- **WoW:** the default UI is built from the same addon API as third-party addons. Mod windows look native because
  they are made the same way. → Our own Extras windows will move onto the mod API (dogfooding).
- **Factorio:** mod GUIs are trees of the base game's element types with the game's named styles. → Mods describe
  windows as a tree, and we draw it.
- **BG3 Script Extender:**
  - It offers an ImGui layer, the easy path, so BG3 mod windows look foreign. → Avoided.
  - Engine objects held past their callback raise "lifetime expired". → Our lifetime rule for handles.
- **Skyrim (Papyrus, SKSE):**
  - Script state in save games breaks when mods change. → We persist plain data only.
  - A fixed per-frame script budget makes scripts lag under load. → Budget per mod and make overruns visible.
  - Native plugins bind raw addresses and break on every game update. → Our API is a façade, not internals.
- **All of them** ended up with declared settings drawn by the game (WoW Settings API, Factorio mod settings, Skyrim's
  MCM, BG3's MCM). → Mods declare settings; we draw them in both themes.

## 3. Architecture

### 3.1 Runtime

- **mlua with Lua 5.4** (vendored; `lua54`, `serde`, `macros`), moving to 5.5 later.
  - Integers and bit operators suit FFXI ids, flags and packet fields.
  - Count hooks give time budgets, and memory limits work per VM.
  - LuaLS gives editor completion from our generated stubs.
  - LuaJIT is rejected: its FFI is a sandbox hole, and hooks are unreliable under the JIT. Windower (5.1) and Ashita
    (LuaJIT) add-ons are ported, so syntax differences cost little.
- **One VM per mod.**
  - This gives each mod its own memory limit, instruction budget, crash containment and hot reload.
  - Every command a mod issues carries its id.
  - Mods talk to each other by copying values: `mods.call(name, fn, ...)` and custom events.
  - VMs live on the main thread; there is no cross-thread Lua.
- **Every handler runs as a coroutine.** `sleep(seconds)`, `sleep_frames(n)` and `wait_until(fn)` work anywhere, as
  Windower and Ashita add-ons expect (24 and 16 add-ons sleep inside handlers).
- **Libraries:**
  - Our explicit set: base (without `dofile` / `loadfile`), `table`, `string`, `math`, `utf8`, `coroutine`.
  - Plus our own `fs`, `json`, `time`, `bit` helpers, and `string.pack` / `unpack`, which Lua 5.4 already has.
  - `io`, `os` and `package` are not loaded. `require` resolves inside the mod's folder and the shared lib folder.
  - `load` takes text chunks only, for mods that evaluate user files (GearSwap job files, eval).

### 3.2 Where mods run in a frame

```
frame (main thread)
├─ net: raw packets in → INCOMING PACKET HOOKS (chain) → decode → game state
├─ game step (actors, event VM, input)
├─ MOD PHASE: queued events per mod, timers and coroutine wake-ups, accounting
├─ apply mod commands (through the paths keys and text commands take)
├─ UI: built-in windows + mod widget trees (kit in Modern, retail painter in Classic)
├─ outgoing packets built → OUTGOING PACKET HOOKS (chain) → net thread
└─ render
```

Some hooks run synchronously inside the pipeline: packets, chat lines, typed text and input. They are filters: they
see the value, may change or block it, and have a tight budget. Everything else is delivered in the mod phase, so Lua
never runs deep inside game code with state half-changed.

### 3.3 Net pipeline change (needed for packet hooks)

The net thread decodes s2c packets into `NetEvent`s and encodes `NetCommand`s (client/net.rs). Built: a round trip
through the main thread, only while some mod hooks that direction (an atomic flag the mod phase sets):

- **Incoming:**
  - The net thread sends each received batch, undecoded, as `NetEvent::RawIn`; the main thread runs the hooks and
    returns the result as `NetCommand::Decode`, which the net thread decodes before its next receive.
  - Injected incoming packets go through the hooks and the same decode, exactly as if the server had sent them.
- **Outgoing:**
  - The 400 ms batch goes out as `NetEvent::RawOut` and comes back as `NetCommand::SendBatch`, sent as it is.
  - Each packet passes the hooks with `id`, `data`, `original`, `blocked` and `injected`.
- **Sessions:** each batch carries its zone session's number; one returning after a zone change is dropped. While a
  batch is away, later ones follow it through the main thread, so the order holds when the hooks go away.
- **Ordering:** a hook may insert packets before or after any packet in the batch. This is GearSwap's "equips ahead of
  the action, midcast equips right behind it".
- **Chaining (Windower semantics):** each hook sees the original and the modified bytes. A blocked packet still
  reaches later hooks with `blocked = true`. Injected packets come back through the hooks with `injected = true`.
  `packets.last(dir, id)` returns the last packet seen of each id.

### 3.4 Mod packages

- **Folder:** `~/.local/share/vana-client/mods/<id>/`, beside the DAT overlay mods.
  - A Lua mod has `mod.toml` and `init.lua`.
  - A folder may also carry DAT overlays (one mod can ship both).
- **`mod.toml`:**
  - Identity: `id`, `name`, `version`, `api` (integer), `authors`, `description`.
  - Loading: `dependencies` (required / optional, with version ranges), `load_after`, `lobby` (the VM lives from
    process start instead of from login), `entry`.
  - `permissions`, `settings`, and localised strings.
- **Lifetime:** the VM is created at character login and dropped at logout; per-character data loads with it.
- **Hot reload:** per mod, keeping storage. `/lua reload <mod>`, plus a developer mode that watches the folder.
- **Errors:**
  - Every call is guarded with a traceback.
  - Each distinct error appears once, as a log line in error colour with mod and file:line; the full text goes to
    vana-client.log.
  - A mod that keeps failing, exceeds memory or hits the hard budget is **suspended**: handlers off, windows hidden,
    storage kept, and a notice.
- **Accounting on F12:** time per frame (average, peak), instructions, memory, errors per mod.
  - The soft warning is 2 ms per callback, the hard cap 50-100 ms; both to be tuned by measurement.

### 3.5 Permissions

Mods ask for permissions in the manifest; the user grants them on the Mods page when enabling the mod, and can see
and revoke them there.

| Permission | Grants | Needed by (examples) |
| --- | --- | --- |
| *(none)* | read state, own windows and HUD, own settings / storage, chat output, own commands, sounds | most trackers, timers, HUDs |
| `actions` | game actions from the mod's own UI or commands, in response to the player's input | click-to-cast (Nostrum, Rhombus) |
| `automation` | game actions on the mod's own initiative: cast, target, move, trade, lot, NPC menus | GearSwap, AutoRA, Treasury, HealBot, FastFollow |
| `packets.read` | raw packet hooks, read only | 58 / 106 add-ons |
| `packets.write` | modify, block, inject | battlemod, DressUp, organizer, GearSwap |
| `input.capture` | swallow keys and mouse, synthesise keys | Tab, Yush, autoenterkey |
| `files.shared` | read and write outside the mod's folder, in a shared data area | Logger, findAll, organizer |
| `net` | HTTP(S), UDP and TCP, open URLs | Pricer, CurePlease, Guildwork-style tools |
| `exec` | run programs | Rich Presence helpers, update |
| `multibox` | the local bus between clients, window focus | send, switch_focus, FastFollow |

Never exposed to any mod, not even read-only: the password, the 2FA secret, `secret.key`, session keys and the
Blowfish state. Outgoing chat, commands and packets are rate-limited per mod. There is no WoW-style taint or combat
lockdown: on a private server the player decides.

## 4. The API

Every namespace below maps to taxonomy rows in the surveys. Names are a draft; a Rust registry generates the bindings,
the LuaLS stubs, a JSON description and the Markdown reference.

### 4.1 `mod`, `mods`
Identity, path, version, `require` of own files, `mods.list()` with state and memory, load / unload / reload of other
mods (manager mods), and `mods.call` / `mods.provide`. A mod can also provide a **service** to other mods: Windower's
Timers plugin is driven this way by 8 add-ons.

### 4.2 `events`
`events.on(name, fn, {filter, priority})` returns a handle. Order is by priority, then load order. A return value
never cancels other mods' handlers; only filter hooks can block.

- **Lifecycle:** `load`, `unload`, `reload`, `login`, `logout`, `zone_change`, `zone_ready`, `settings_changed`.
- **Derived game events:** Windower's 30 change events, plus what Ashita add-ons decode by hand.
  - Vitals: `hp` / `mp` / `tp` and their percents and maxima, `status_change`, `job_change`, level up / down,
    experience gained / lost.
  - Effects: `buff_gain` / `buff_lose` (self and party), `target_change`.
  - Combat: `action` (decoded 0x028), `action_message` (0x029).
  - Social: `party_change`, `party_invite`, `linkshell_change`, `examined`, `emote`, `chat_message` (sender, mode,
    GM).
  - Items: `item_add` / `item_remove` / `item_change`.
  - World: `time_change`, `day_change`, `moon_change`, `weather_change`.
  - Server-driven: `treasure_pool`, `event_start` / `event_end`, cutscene, `menu_open` / `menu_close` (retail names),
    widescan results, synthesis results, RoE progress, and the music change.
- **Input:** `key`, `mouse` (filters).
- **Per frame:** `frame` and `postrender`, accounted. Retained widgets and HUD elements make most per-frame polling
  unnecessary.

### 4.3 `chat`
- **Write:** `chat.add(mode, text)` with colour codes, auto-translate, and our formatter's in-line colours. Lines are
  indistinguishable from game lines.
- **Filter hook, upstream of display *and* behaviour:**
  - Each line comes with its raw FFXI bytes (0x1E / 0x1F colours, 0x7F codes, auto-translate blocks), mode and indent.
  - A hook may rewrite the text and mode, or block the line.
  - Removing 0x7F 0x31 must make NPC dialogue advance (enternity), which means the hook sits before the event text
    path.
- **Typed line:**
  - An outgoing-text filter sees the raw typed line before our command parser, and may rewrite or block it (GearSwap,
    shortcuts, itemizer).
  - `chat.input(line)` types for the player, exactly as typed, `<t>` / `<st>` prompts included.
- **Input line:** read and set the text and caret, whether it is open.
- **Encoding:** auto-translate encode and decode; Shift-JIS conversion for packets.
- **Logs:** the chat log history (read), chat filters, and log windows 1 / 2.

### 4.4 `commands`
- **Mod commands:** `commands.register("name", fn)` for `//name` and `/name`, with aliases, and an unhandled-command
  hook.
- **Running commands:** `commands.run("a; wait 1; b")` takes the same path as typed text: game commands, our native
  commands, and other mods' commands. `/lua` is the player's only: `commands.run` refuses it, and a mod's bind
  line or action that reaches it is refused too, so a mod can't grant itself permissions or run code in another.
- **Binds:** `binds.set(key with modifiers, command, {down/up, chat-open})` and `unset`. Binds also appear in our key
  assignment.
- **Scripts and macros:** script files and `/exec`; macros (read books, pages and lines, run them, set the book / set).

### 4.5 `input`
Key and mouse events at two levels: text entry and game controls. Mods can swallow events (permission), read modifier
and held-key state, and synthesise keys. Gamepad events and injection come with gamepad support (deferred). Keys have
names, not DIK or VK codes.

### 4.6 `game`: state, as a façade
Our internal structs are not the contract. The façade comes in three forms:

1. **Handles:** stable keys such as entity id plus spawn generation, or bag plus slot. They are safe to keep, resolve
   against the current state, and have `valid`.
2. **Scoped views:** zero-copy reads during a dispatch. Using one after its callback is an error, not a crash.
3. **Snapshots:** `game.snapshot(area)` returns plain tables (serde) for the long tail of "nearly all state".

Areas, covering both surveys' state rows (Windower 13.4, Ashita 8.5):
- **Player:** identity, jobs and levels, vitals and maxima, stats and resists, buffs with timers, skills, merits, job
  points, master levels, exp / limit / capacity, item level, rank, nation, titles, currencies, unity, status, speed,
  and the pet (hp, mp, tp).
- **Party and alliance:** members in and out of the zone, vitals, jobs, buffs, leaders, flags.
- **Entities:** every one in range by id or index, plus zone NPC lists.
  - Name, position, heading, distance, hpp, status, claim, spawn type, model and look, size, speed.
  - Target, pet and fellow indices, render flags, owner.
- **Target:** target, sub-target (and whether it is active), lock, the battle target and other special targets
  (`<bt>`, `<ht>`, `<lastst>`, `<scan>`, `<r>`), and the focus target.
- **Items:** every bag (id, count, raw extdata and decoded augments / charges / timers), bag sizes, equipment,
  wardrobe access, the treasure pool with lots, the item shown in the help window.
- **Actions:** learned spells, abilities, weapon skills, pet commands, mounts; recasts and charges; the cast bar.
- **Job data:** the BLU set, PUP attachments and automaton, MON instincts.
- **Key items, RoE, missions, quests** (our journal data).
- **World:**
  - Zone and sub-area, zone flags (Mog House, wardrobe, conflict), server, language.
  - Vana'diel and earth clocks, day, moon, weather.
  - Event / cutscene active, map coordinates and `<pos>`, widescan list and tracking.
- **UI state:** open windows by retail name (the stack), the menu cursor, UI hidden, chat open / expanded, log
  windows, the help line, and the input line.
- **Camera:** position, orientation, fov, view and projection matrices, mode (first person, locked); world-to-screen.

Where an add-on wrote memory instead of reading it, the write becomes a call in `render` (4.11), `ui.native` (4.10)
or `actions` (4.8).

### 4.7 `packets`
- **Hooks:** `packets.on_in(id or "*", fn)` and `packets.on_out(...)`, with the semantics of 3.3.
- **Changes:** block, modify, inject both ways, and batch ordering.
- **Layouts:** `packets.parse` / `packets.build` / `packets.new` from layouts for every id our client knows. Windower's
  221 layouts are the de-facto schema; field names follow LSB / Windower where they agree.
- **Decoded views** of the hot ids (0x00A, 0x00B, 0x028, 0x029, 0x076, 0x063, inventory) take most mods off raw bytes.
- Needs `packets.read` / `packets.write`.

### 4.8 `actions`
Direct game actions, all through the same code the player's own actions take:
- **Combat:** cast, use an ability, weapon skill or item, ranged attack, attack and disengage, target, assist.
- **Items:** equip and equip sets; move, sort, drop and stack items, get from and put into storage.
- **Treasure and buffs:** lot and pass; cancel a buff.
- **Job setups:** set and reset BLU spells and PUP attachments.
- **World:** widescan tracking; trade, sell and buy; answer NPC menus (event options through 0x05B / 0x05C).
- **Social:** party invite and reply, mounting.
- **Movement:** run in a direction or to a point, turn, follow, walk toggle, stop. These are synchronised with our own
  movement and autorun code.

Requires `actions` or `automation`.

### 4.9 `res`
- **Static tables**, generated from our DAT readers:
  - items, spells, abilities, weapon skills, statuses, zones, key items, jobs, races, mounts, titles, merits
  - action messages, auto-translate, music, weather, days, moons, augments, monster abilities
- **Formats:** names in each language, and Windower's field conventions where ports benefit.
- **Icons as UI images** for items, statuses, jobs and spells, usable in widgets and the HUD.
- **Game text:** dialogue text by zone and id, and zone NPC lists.
- **Raw DAT bytes** by file id, read-only and through the DAT overlay (equipviewer, boxdestroyer).
- **Bundled data:** mob databases and similar are the mod's own data files.

### 4.10 `ui`: widgets, HUD and native windows
- **Windows:**
  - A retained tree the mod builds and updates. Rust draws it every frame: the kit in Modern, retail sprites and
    fonts in Classic. Lua runs only on events.
  - Windows take part in window groups, Escape, the UI-hide key and per-character saved places.
  - A window can attach to a built-in one ("right of the item list when it opens").
  - A mod can add a row to a retail menu or the Mods page.
- **Element set** (from Ashita's ImGui inventory, 103 add-ons, and our pages):
  - Text: labels with roles, the help line, tooltips, (?) help markers.
  - Controls: buttons and button rows, toggles, options, sliders, number and text inputs (single and multi-line),
    combos.
  - Collections: lists with selectable rows, tables with columns, sorting, resizable columns and frozen headers,
    trees and collapsing sections, tabs.
  - Overlays: popups, modals, yes / no prompts, right-click menus.
  - Media: colour picker, icons and images, gauges / progress bars.
  - Layout: separators, indents, scroll regions, and canvas regions for custom drawing (lines, rects, circles,
    gradients, clipping).
  - Behaviour: the kit owns keys, mouse, cursor, scroll bars and sounds. Styles are by role (title, row, dim, value,
    warn), resolved per theme. Some elements need adding to the kit and to the retail painter first.
- **HUD layer** (Ashita's 36 borderless overlays, Windower's texts / images libs):
  - Retained text, image, bar and rect elements, anchored to the screen or to native windows, movable, with saved
    places.
  - Fonts, outlines, backgrounds and inline colours.
  - Textures from the mod's files and game icons.
  - Hit-testing and drag; GPU-batched; hidden while a menu is open when asked.
- **World space:**
  - Lines, rings, boxes, quads and text placed in world coordinates, depth-tested against the scene.
  - World-to-screen projection for 2D labels.
  - These cover ZoneLines, HitboxDisplay and GEO-HUD-style ground rings.
- **Native windows (`ui.native`):**
  - Show or hide each built-in window, or the whole interface.
  - Open a game menu by name, and know which are open.
  - Read a native list's rows and cursor, and send it the keys the game would (ahpro).
  - Advance dialogue (stepdialog, enternity).
  - Read and set the game config by id.

### 4.11 `render`
- **Engine settings mods may change at run time:** draw distance (scene and mobs), aspect, frame limit (FastCS-style
  cutscene rate), ambient light, gamma, wireframe, clear colour, footsteps, animation rate.
- **Camera:** distance, battle distance, pan speeds.
- **Per-entity overrides:** visibility and occlusion, alpha, tint, name, name colour, model size, look slots, race and
  face, effect bubbles.
- **World:** music override or mute per context, weather shown, screenshots.
- Shader-level changes (toon, cel shading) are a later shader-pack interface, not Lua.

### 4.12 `settings`, `storage`, `fs`, data formats
- **Settings:** declared in `mod.toml`, drawn by us in both themes under Mods.
  - Types: bool, int, number, enum, string, colour, key, list.
  - Each has a scope: account, character, or server plus character. Changes raise `settings_changed`.
- **Storage:** plain data per scope, versioned, with a `migrate` hook. Saved at logout, on reload and periodically.
- **Files:** `fs` inside the mod's folder (read, write, append, list, mkdir, remove, rename); a shared area with
  `files.shared`.
- **Formats:** JSON, XML and CSV helpers; SQLite (rusqlite) for the few that need it.

### 4.13 `timer`, `time`
- `sleep` and `sleep_frames` in any handler; `timer.after` and `timer.every`, by time or frames.
- Clocks: monotonic, real, and Vana'diel.
- A shared timer-bar service, through Timers, for Windower's Timers-plugin pattern.

### 4.14 `audio`
Play the mod's sound files (WAV, OGG, MP3) through our mixer with volume; play game sound effects by id; override or
mute music. The chime.

### 4.15 `ipc`, `net`, `client`
- **Between mods:** a message bus.
- **Between clients on one machine (`multibox`):** a message bus keyed by mod id, commands to `@all`, `@others` or a
  character, and window focus.
- **Network (`net`):** async HTTP(S), UDP and TCP, open a URL.
- **Client:** window geometry, focus, the clipboard, screenshots, and running programs with `exec`.

## 5. Proving the bar

- **Coverage table:** the surveys' scan data gives every add-on and plugin as a row: its operations mapped to API
  calls, and a status (covered / partly / missing). It is generated from the add-ons' sources, committed beside this file, and re-run
  as the API grows. A missing row is a bug in the API.
- **Reference ports,** written as real mods, chosen to cover the hard classes:
  - **GearSwap:** the platform, the 20 callbacks, a stock Mote-libs job file.
  - **battlemod:** filtering and rewriting combat text.
  - **XIUI-like HUD:** overlays, icons, native element hiding.
  - **Treasury / organizer:** item automation, NPC menus through packets.
  - **enternity:** dialogue control codes.
  - **DressUp:** entity visual overrides.
  - **FastFollow:** movement, multibox.
  - **ahpro-like:** driving a native window.
  - **ZoneLines:** world-space drawing.
  - **Timers:** a service used by other mods.
- **Dogfooding:** at least one of our own Extras (`/dps`, the training regime window or Timers) moves to a bundled
  Lua mod, drawn in both themes.

## 6. Phases

1. **Runtime:** mlua, a VM per mod, manifest, permissions page, limits and budget, errors, F12 accounting,
   `/lua` commands, storage and settings, coroutine scheduler, LuaLS stub generator.
2. **Events and state:** lifecycle and derived events; `game` façade for player, party, entities, target, items,
   recasts and world; `res` with icons; chat write and filter; commands run and register.
3. **UI:** widget tree with the kit elements in Modern and the matching Classic renderer; HUD layer; Mods page
   (settings drawn); one Extras feature ported.
4. **Packets:** the net pipeline change (3.3); hooks, chain, inject, batch order; layouts and decoded views.
5. **Actions and automation:** `actions`, movement, NPC menu answers, `ui.native`, `render` overrides.
6. **The rest:** world-space drawing, audio, ipc / net / client, multibox.
   - Then the reference ports and the coverage table to 100%.

## 7. Decisions (user, 2 November 2026)

1. **Lua 5.4** through mlua. Not Luau, not LuaJIT.
2. **Permissions:** the table in 3.5 as it stands, with `automation` a single grant.
3. **Mods folder:** shared with the DAT overlay mods: `mods/<id>/`, and one package can carry both.
4. **Our Extras become mods:** every Extra moves onto the mod API over time and ships as a bundled mod, so our own
   features prove the API (WoW's model).
