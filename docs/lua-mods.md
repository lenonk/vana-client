# Lua mods: design

Writing mods (tutorial, guide, reference, examples): [modding/](modding/README.md). This document is the design and
the state of the work.

The add-on surveys behind it are in MODLOG "Lua mods: the add-on surveys".

## State

Built (client/lua/, modern/modwin.rs), each checked live:
- **Runtime:**
  - a VM per mod (Lua 5.4, mlua 0.12): 38 to 64 KB each, measured on F12
  - restricted libraries, text-only `load`, a 64 MB memory limit, no `__gc` finalizers (they run with hooks off)
  - a 100 ms budget per call (a runaway loop is stopped; string patterns count their steps against it too)
  - coroutine handlers (`sleep`, `sleep_frames`, `wait_until`)
  - errors reported once with file:line; 20 in a minute suspend the mod
  - `require` from the mod's folder (no links out of it)
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
  - an entity's `target_index` / `target_id` (its face target from 0x00D / 0x00E; ours our target; LSB fills it for
    players only, never a monster's battle target), our pet's `mpp` / `tp` (s2c 0x068), `game.cast` (our cast bar:
    spell, total, left, progress); checked live (our target, a Cure's bar)
  - `game.macros` (the palette's book, set, titles and the current set's 20 macros) and `actions.macro` /
    `macro_set` / `macro_edit` (run one as its key does, switch book and set, change a line or title and save);
    checked live
  - `res.item`, `spell`, `ability`, `status`, `zone`, `title`, `key_item`, `job`, `augments` (an item's extdata
    decoded as the item window shows it: lines, the short line, message id / value pairs; checked live)
  - `res.message` (a battle / system message expanded as the log prints it, with actor, target and values),
    `mob_skill`, `skill`, `day`, `moon`, `weather` / `weather_element`, `element`, `mount`; checked live
  - the zone's collision: `game.collision(x, z, radius)` (triangles near a point with normals and surface kinds),
    `game.collision_mesh` (the whole mesh packed for export), `game.raycast`, `game.floor`; checked live (Port Jeuno:
    the floor under us, a ray down and across the corridor to its walls)
  - `res.name(kind, id, lang)` (item, key item, spell, ability, zone and title names in ja / en / de / fr; another
    language loads on a thread, nil until in), `res.dat` / `dat_path` (a file's bytes through the DAT overlay, and
    where it is read from), `res.model_files` (an NPC model's file, a look's skeleton and part files); checked live
    (Japanese item, zone and spell names, a DAT read, a look and a model resolved)
  - `res.autotranslate(word?, group?)` (the dictionary's entries: text, group, id and the phrase as a string) and
    `autotranslate_groups`; `text.phrase(group, id)` and `text.autotranslate(text)` (an exact entry) give a phrase to
    put in `chat.send` / `chat.add` / `/echo`, which print it braced as the log does; checked live
  - events `emote` (s2c 0x05A), `party_invite` (0x0DC), `examined` (message 89); an entity's `hitbox` (its model
    radius, ours from 0x037); emote and hitbox checked live, invite and examined not (they need a second player)
- **Chat and commands:**
  - `chat.add` (mode colours)
  - `filters.chat` (keep, drop or rewrite every line) and `filters.typed` (typed lines before the parser)
  - `commands.register` (`//name`, `/name`), `commands.run`
- **Settings and storage:**
  - declared settings (bool, enum, int, number) on an Extras page per mod, in both UIs; per-scope files
  - `storage.account` / `storage.character`
- **Timers** (`timers.add` / `remove` / `clear`): a mod's rows in the Timers window (recasts, buffs or debuffs,
  under a heading, with a status, spell or ability icon), shown even with the Timers Extra off; dropped when the mod
  unloads. Checked live in both UIs.
- **Windows:**
  - `ui.window` / `update` / `get` / `open` / `show` / `close` / `on` / `prompt` / `menu`
  - elements: text, buttons and options, slider, scrolling list, gauge, separator; tabs (elements on pages), typed
    lines (edit / change events, Escape undoes), choices (Left / Right), colours (RGBA, a channel at a time; the
    classic UI shows channel sliders, the modern UI the kit's picker), folding sections, list columns with headings
    (a click sends sort), right-aligned cells, row icons (item, status) and colours; a tip on any element (the help
    bar in classic, the help line in modern); a canvas element the mod draws in (lines, rects with gradients or
    outlines, circles, triangles, text, icons; clicks report where, drags their start, moves and release, both UIs
    checked live); coloured text; modal windows, `ui.prompt`
    (a question with buttons) and `ui.menu` (a popup list at the pointer, closed by a click outside it); checked
    live in both UIs
  - keys and mouse in both UIs: the classic UI draws Config pages, the modern UI kit panels with saved places
  - HUD windows (no focus, frameless, placed)
  - the HUD layer (`hud`): retained text (size, colour, background, alignment, lines), rects, bars and item / status
    icons in window pixels (negative from the right or bottom edge), drawn under the windows and hidden with the UI;
    text in the active UI's face or a chosen one in either UI (the game font, alpha blended so dark text shows; the
    modern regular or bold), with an outline (colour, width) or a drop shadow (checked live in both UIs);
    `hud.map` (a window onto the zone's map, where the client shows one: centred on us or a world point, its span in
    map pixels), `game.map` (the map's page, file, scale, origin) and `game.map_pos` (world to map image pixels);
    checked live; PNG images from the mod's folder (`hud.image`, sized and tinted); `hud.on` takes left / right clicks on an element from the game; `hud.screen` (size,
    pixels per UI unit), `hud.bounds`. Checked live in both UIs, clicks too
  - (a user's request, 2026-11-06; checked live) spell and ability icons (`kind = "spell"` / `"ability"`, the magic and ability
    lists' sprites) for HUD, list and canvas icons; HUD drags (`drag_start`, `drag`, `drag_end` with the element
    under the release, `drop` to it), `mouse_enter` / `mouse_leave`, and a `tip` on any HUD element (the kit's tooltip
    in modern, a box by the pointer in classic); a click now comes at the release; `game.recasts().totals` (each
    running recast's whole length, as it started) and `res.ability(id).recast` (LSB's base recast, bundled:
    data/recasts.txt)
- **Native windows** (`ui.native`): the game's open windows by name and whether one has the keys; the whole
  interface hidden or shown; HUD pieces hidden in both UIs while the mod runs (party, target, buffs, compass /
  location, minimap, cast bar, name plates, the alliance lists alone, the log windows (back while an event, a prompt
  or the chat line needs them), Timers, the training regime window; undone when it unloads); game config values read and set through the
  Config window's apply (`ui.native` for changes); `actions.advance` moves event text on. Checked live in both UIs.
  `ui.native.highlighted` (the item, spell, ability, mount, emote or key item under the cursor of the window with the
  keys, an item's bag and slot) and `ui.native.list` (that list's rows, cursor and top: the item lists and storage,
  magic, abilities, key items, shops, the auction house's lists and menus), driven with `input.press`; checked live
  on the item, ability, weapon skill and key item lists (modern UI); shops and the auction house not tried live.
  `ui.native.item_colour` colours an item's name in the shop and auction lists (checked live in the AH's crystals).
  Changing the built-in UI, as WoW's add-ons do (user, 2026-11-06): in the modern UI every panel that never takes
  the keys hides by name; `ui.native.place` moves, scales (an egui layer transform about a chosen corner, the pointer
  with it) and fades any panel but the edge windows; `ui.native.rect` / `panels` say where each is (a hidden one where
  it would stand now); a HUD element's `anchor` places it from a panel's corner (a replacement where a hidden one
  stands, or beside a shown one: HUD elements draw under panels); `ui.skin` replaces the pack's sprites (keeping their
  logical size and insets), palette colours and window frames (border and gradient). Each undone when its mod
  unloads. Checked live (modern UI) with the action-bar and panel-tweaks examples: icons, tooltips, a swap and a bar
  moved by drags, a Berserk recast's total, a scaled target window, the location panel replaced, the skin on and off.
- **Render** (`render`): per entity, while the mod runs (the newest mod's wins): hidden (model and plate), plate name and
  colour, alpha, tint, size; the camera's distance read and set; `render.camera` / `set_camera`: the drawn eye's
  distance (a share of retail's, up to 10), the pan speed's scale, a distance while engaged. Checked live (a renamed, tinted, doubled NPC,
  a hidden one, ourselves translucent, the camera pulled back and out to 6); the engaged distance not seen live.
- **Audio** (`audio`): the mod's WAV files (16-bit PCM, any rate) through our mixer at the effects volume, the game's
  sound effects and interface sounds by id; the music held on a track or silenced and given back (`audio.music`,
  `mute_music`). Checked live (a WAV, a game effect, an 8-bit file refused; track 115 over the zone's, muted, back).
- **World space** (`draw`): lines, rings (outline or filled) and wire boxes in entity coordinates, drawn after the
  effects and tested against the scene's depth (no writes); `draw.to_screen` for labels (window pixels, as `hud` takes).
  Checked live (rings at our feet, a line and a box at an NPC, a label over her head). `draw.text` (text at a world
  point) and `draw.image` (a PNG of the mod's on a world quad: on the ground, upright toward a yaw, or turned to the
  camera; in 8 x 8 pieces for its perspective), both drawn with the HUD behind what stands in front; checked live (a
  circle under us, a label over us, an upright and a camera-facing panel).
- **Network and other windows:** `net.request` (HTTP(S) on its own thread, the answer to a callback in a later mod
  phase; `net`), `net.open_url`; `ipc.send` and the `ipc_message` event between game windows on one machine (local
  UDP, ports listed in the runtime folder; `multibox`). Checked live against a local server and a stand-in window.
  Sockets for companion programs (`net`): `net.connect` / `listen` (TCP out and in, 127.0.0.1 unless asked),
  `net.udp`, `net.pipe` (a Unix socket or a Windows named pipe another program offers, such as Discord's IPC),
  `net.send`, `net.close`; reading on threads, handlers (open, data, accept, close) in later mod phases.
  Checked live (a listener with an echo, UDP both ways, a refused connection).
- **The game window and the system:** `client.window` (place, size, focus) and the `focus_change` event;
  `client.set_window` (move, size through the Settings' sizing path; always on top, minimised, borderless full
  screen, the frame, kept out of screen captures) and `client.focus` (`multibox`);
  `client.screenshot` (under the screenshots folder, subfolders allowed); `client.paste` (`clipboard`) and
  `client.copy`; `client.run` (a program without a shell, its exit code and output to a handler; `exec`);
  `client.option` / `set_option`: behaviour switches while the mod runs, retail's back when it unloads (the macro
  palette's wait for the next whole second, the AH bid result's wait, the 0.9 s between equip sets, the log's slide,
  the cutscene speed 1..4, the actors' animation rate, the scene's background colour and ambient light, the minimap's
  zoom; set, read and released checked live, the ambient light and the minimap's zoom seen, the rest not all seen). Checked
  live but `client.focus` (not tried: it would take the desktop's focus); the place reads nil under Wayland.
  - `devices.list` / `open` / `write` / `close` (`devices`): HID devices by their raw reports (G-keys, Stream Decks),
    hidraw on Linux, the HID interfaces on Windows; checked live on Linux (the list with ids, interfaces and report
    descriptors; a device opened and closed); Windows type-checked, not run
  - `client.record` / `stop_recording` / `recording` (`exec`): the window as drawn to an H.264 file through ffmpeg
    (a ring of readback buffers, never waited on; a frame due with none free repeats the last); checked live (4 s at
    30 fps: 120 frames, 1920 x 1080)
  - `render.shader(id, wgsl)` / `shader_params`: post-process passes over the world under the fades and the 2D layer,
    each a WGSL `fs_main` given the frame so far, its depth, size, time and four parameter vectors; a WGSL error is a
    log line (its line numbers count the 15 header lines first) and the pass is skipped; checked live (a cel-shading
    pass with depth outlines; an error reported)
  - `client.process` / `set_affinity` / `set_priority` (`process`): the cores every thread may run on and the
    scheduling priority; checked live on Linux (cores 0-3, low, back to normal).
  - Rules as switches too: `move_lock` (our abilities' and the weapon draw's move lock, scheduler op 0x2e), `all_maps`
    (every map without its key item), `dead_commands`, `compass_monsters` (monster dots for every job),
    `player_collision`, `occlusion` (characters behind scenery), `menu_reach` (the AH and the nomad moogle menu
    closing 6 away), `send_interval` (ms between our datagrams, retail 400; checked live: 10 in 4 s at 400, 38 at
    100), `wireframe` (the scenery as lines where the GPU has line polygons; seen live), `ttf_text` (the game font's
    text in the TrueType face in the classic UI: the log, windows' text; seen live). A value set shows in `client.option` from the next frame. Checked live: Berserk held the walk
    with `move_lock` on (4.2 yalms in 1.5 s) and not with it off (7.5), and `all_maps` gave the Port Jeuno map without
    its key item; the rest not tried live.
  - `actions.open_menu(name)`: a main menu row by name (status, items, magic, map, config ... log_out), run as a pick
    there; `moogle` the nomad moogle menu. Checked live with status.
- **Permissions:** each one a mod asks for is a row on its Extras page (ON / OFF, with what it allows on the help
  line), or `/lua grant` / `revoke`; kept in mod-data/grants.json. A mod loaded without them says where to allow them.
- **Packets** (`packets.read` / `packets.write`):
  - `packets.on_in` / `on_out` (one id or all) see each packet before the client reads it or after it built it, as
    a chain: original and current bytes, blocked, injected; a handler returns new bytes, `false` to block, or nothing
  - `inject_in` (decoded as if the server sent it) and `inject_out`, both through the hooks; `build`; `last`. A
    packet's size field is set from its bytes (injected or returned by a hook); one over 508 bytes (the 7-bit field
    of 4-byte words) is refused
  - checked live: counts by id, an injected tell, a say rewritten into a GM command the server answered, across zones
- **Actions** (`actions`: only while handling the player's input, a typed command or a window event; `automation`:
  any time; a coroutine keeps how it began across sleeps; `commands.run` of a game command is an action too):
  - `cast`, `ability` / `weaponskill`, `ranged`, `use_item`, `use_slot`, `attack`, `disengage`, `assist`, `target`,
    `lock`, `heal`, each through the menus' and text commands' own code (checks, refusal lines, packets)
  - `equip` (one piece or a set table, by id or bag slot; worn pieces stay), `unequip`, `equip_set`, `move`, `sort`,
    `drop`, `lot`, `pass`, `cancel`, `choose` (an event menu option), `talk`, `scan`, `track`, `untrack`
  - movement on autorun: `run`, `run_to`, `follow`, `walk`, `stop`, `turn` (headings as entity.heading)
  - `trade` (items and gil to an NPC within reach), `buy` / `sell` with the open NPC shop (`game.shop`), `blue_set` /
    `blue_remove`, `attach` / `detach` (automaton), `invite`, `party_leave`, `party_kick`, `join`, `decline`, `mount`,
    `dismount`, `advance`; trade and shop refusals are the mod's errors. Checked live: a bad trade refused, Eye Drops
    bought from Gekko and sold back
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
  - `mods.list` (with each mod's time a frame, memory and errors), `mods.installed` (every mod on disk and bundled:
    enabled, loaded), `mods.provide` / `mods.call` (a function in another mod's VM, values copied; not a suspended
    mod's), `mods.emit` (an event to every other mod); `/lua hide` / `show` hides every mod's HUD, windows and world
    drawing at once
  - `text.from_shift_jis` / `to_shift_jis` / `plain` (auto-translate phrases as words); `client.copy`
  - `chat.icon(png)`: an inline image as a character in chat text, drawn in both UIs' logs (the classic UI's game-font
    text keeps the colour in force across it); checked live in both UIs
  - a JSON null reaches Lua as nil everywhere (event arguments, tables, `json.decode`, `mods.call`)
- **Lobby mods** (`lobby = true` in mod.toml): `lobby.state` (the screen, the server and account, the character list),
  the `lobby_screen` event, `lobby.sign_in` (as Log in) and `lobby.play` (as the select screen's Play), keys through
  `input.press`, timers, sockets; `storage.seal` / `unseal` keep a password that only this mod on this machine can
  open. Checked live: an autologin port signed in with its sealed password and played a character, stayed loaded
  through the world and logout, and was unloaded as the client closed.
  - checked live: a call returning two values, an emitted table, files written, appended, listed and read back, the
    mod's own file read, `..` and `shared/` refused, JSON both ways, the os subset, Shift-JIS round trip
- **Bundled mods:** `/dps` (the first Extra ported).
- **Mods folder:** `mods/<id>/` (enabled through config `mods`); a test run uses `mods` beside its FFXI_CONFIG copy.

Next (docs/lua-mods.md 6): packet layouts and decoded views, batch order (the rest of phase 4); the rest of phase 5
(engine settings such as draw distance and frame limit); the other Extras as bundled mods; then the gaps in
docs/lua-coverage.md.

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
- **Lifetime:** the VM is created at character login and dropped at logout; per-character data loads with it. A
  `lobby` mod is created on the login screen and lives until the client closes: it gets `logout` but stays loaded,
  its `storage.character` and character settings load at each login, and in the lobby no world is lent (game state
  errors; its prints go to vana-client.log).
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
| `multibox` | the local bus between clients, window focus, moving and sizing the game window | send, switch_focus, FastFollow, WinControl |
| `clipboard` | read the clipboard's text (writing it needs nothing) | paste helpers, chat tools |
| `ui.native` | hide HUD pieces, the whole interface, set game config values | XIUI, hideui, config tools |
| `process` | CPU affinity and scheduling priority of the client | Affinity |
| `devices` | HID devices by their raw reports | Binder, GBinder, Stream Deck tools |
| `mods` | `/lua load`, `unload`, `reload`, `hide`, `show` from the mod's lines (`commands.run`, binds) | add-on managers, per-job or per-zone loaders |

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
  commands, and other mods' commands. `/lua grant`, `revoke`, `run`, `set` and `watch` are the player's only: from
  `commands.run`, a mod's bind, a macro a mod started or a chat line a mod submitted with a pressed Enter they're
  refused, leading spaces included; `actions.macro_edit` refuses a `/lua` line.
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
- Shader-level changes (toon, cel shading) are WGSL post-process passes a mod sets (`render.shader`).

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

- **Coverage table:** [lua-coverage.md](lua-coverage.md), every add-on and plugin as a row: its operations mapped to
  API calls, and a status (covered / partly / missing), with the open gaps ranked by the rows they block (615 rows, 554
  covered as of 478e408). Re-rated as the API grows. A missing row is a bug in the API.
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
