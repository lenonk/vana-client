# vana-client

A native client for FINAL FANTASY XI, in Rust, that logs into a LandSandBoat (LSB) server. No Wine, no PlayOnline.
Rendering is wgpu (Vulkan), windows winit, audio cpal (PipeWire), UI the game's own menus and sprites (egui only for
input and the F12 debug overlay). Faithful to retail first; improvements come after.

## Where things are
- Code: `crates/vana-client/src` (client/ game logic and UI, render/ GPU, dat/ game file formats, net/ protocol,
  audio/).
- `MODLOG.md`: every reverse-engineered fact (FFXiMain addresses, DAT formats, packets), plus what's inferred.
  Read the relevant sections before changing anything, and append what you learn.
- Game install: `~/.local/share/vana-client/game/FINAL FANTASY XI` (the default, made by the first-start download
  2026-10-22), patched to current retail (30261006_0), code files included. The unpatched 2019 base is in
  `~/ffxi/lab/base-install` for comparison. HD packs: `~/ffxi/dat-backup/removed-pack-files` (manifest.tsv), for
  the mod manager. The user's old config is in `~/ffxi/config-backup/config.json`.
- FFXiMain decompile: `~/ffxi/lab/creatorwork/ghidra/all.c` and `all.asm`; unpacked DLL:
  `~/ffxi/lab/compress_re/unpacked/FFXiMain_unpacked.dll`. These stay outside the repo; never commit them.
- Research specs for upcoming work: `~/ffxi/lab/research/`. `keys_spec.md` covers all 180 key functions.
- References (facts only, never copied text or code): `~/ffxi/lab/XiEvents` (atom0s, AGPL-3.0): every event VM
  opcode with pseudo code (current as of Feb 2022 retail), plus event DAT structures; `~/ffxi/lab/POLUtils`
  (Windower, Apache-2.0): DAT formats (`Wiki/FileFormats.wiki`, `Wiki/FFXIDATFileEncryption.wiki`) and their readers
  and decryption in `PlayOnline.FFXI`. Check them alongside the decompile before reverse engineering a format or op.
- Kuluu (`~/ffxi/lab/kuluu-ffxi`, GPL-3.0: facts only, never code) is far behind us and not a guide; at most a
  source of narrow facts, confirmed in our decompile (`~/ffxi/lab/research/kuluu_survey.md`).
- Current FFXiMain (30261006_0): `~/ffxi/lab/current/` (DLL, unpacked), decompile `creatorwork/ghidra_30261006/`
  (run FixVt.java after analysis, or vtable-only methods are missing).
- Helper scripts: `~/ffxi/lab/tools/` (`ptrfind.py` VA/offset mapping, `keymap.py`, `mkcnf.py`, `append_modlog.py`,
  `delchar.py NAME --account LOGIN [--yes]` deletes a character's every DB row so the name can be remade; dry run
  without `--yes`, refuses while the character has an open session).
- LSB server: `~/landsandboat`. To restart a crashed map server, run from there:
  `setsid -f ./xi_map --log log/map-server-54230.log --ip 127.0.0.1 --port 54230 > /dev/null 2>&1 < /dev/null`

## Build and test
- `nice -n 19 taskset -c 0-11 cargo build --release`, `nice -n 19 taskset -c 0-11 cargo test --release` (cores
  12..15 left free; `-j` alone doesn't do it, rustc's codegen threads still take every core. A busier build trips the
  LSB map server's 2 s tick watchdog while its bots and their chat model run, and it is killed). The user runs
  `target/release/vana-client play`.
- Offscreen tools for checking without a window: `npc-shot`, `char-shot`, `select-shot`, `creator-shot`,
  `zone-shot`, `topbar-shot`, `cs-shot`, `flash-shot` and others (`--help`).
- Live test runs: `play --credentials ~/ffxi/lab/lnxtest01.json --input '<script>'` (steps in client/script.rs).
  Driven windows are titled "[test]" and take real input too.
- The client logs to `~/.local/share/vana-client/vana-client.log` (the run before: `vana-client.prev.log`; test runs
  beside their `FFXI_CONFIG` copy), effect tasks and generators at debug level for now: read the user's run there.

## Rules
- Decompile, don't guess. Mark anything inferred as inferred in MODLOG.
- Per-frame work goes on the GPU (skinning, interpolation, mips, effects, batched UI). No CPU shortcuts.
- No fallback or placeholder UI or screens unless retail has them. Nothing is shown until the real thing can draw.
- Retail limits that only existed because of 2002 hardware (30 fps cap and similar) become settings. The frame
  limit defaults to 300.
- Git (branch `master`): commit after each piece of work has been built, tested and checked. Subagents never
  commit or change the tree; `git status` / `git diff` are fine.
- Live tests use only account lnxtest01 (characters Lnxtest, Lnxcsfix, Tuxedo). Never touch the user's account
  (lenon) or the server DB or settings without asking.
  - Lnxcsfix has GM level 5 (user's OK, 2026-10-22): `key:Space;type:!pos X Y Z;key:Enter` in a script moves it.
  - Tuxedo logs in to the "Start Seekers of Adoulin?" prompt (login event 0x36e); a script must answer it.
  - For DB reads, use a 0600 defaults file (`~/ffxi/lab/tools/mkcnf.py`) and delete it afterwards.
- Every live world run ends with a clean logout (`--quit`), with a timeout long enough for the 30 s logout.
  - Never kill a logged-in client: stale sessions cause "already logged in".
  - Stop on that message; don't retry in a loop.
- One test window at a time, killed by PID only, never pkill. Check `ps` first: the user may be playing in their
  own window. Use a config copy (`FFXI_CONFIG`) and audio off (`FFXI_AUDIO_DEVICE=null`: silent, mixed at real
  time; ALSA's `alsa:null` is not paced, spins a core and ends every sound at once) for test runs.
- Before profiling, check that the CPU and the GPU (an eGPU over Thunderbolt) are otherwise idle.

## Next up
- The retail key set (`~/ffxi/lab/research/keys_spec.md`; its status column is out of date): done, including
  F1-F12 targeting, UI hide, screenshots, the Pause log-out prompt, macros and palettes. Left: Mog House layout
  mode keys, Ctrl / Alt page modifier (0xa8) in the lists other than items (done there), Shift selection in the
  chat line.
- Event VM gaps: audit `~/ffxi/lab/research/xievents_check/STATUS.md` (2026-10-23, ranked R1..R21 against
  REPORT.md). Done since: R1..R7, R10, R14, R16..R18, R21. Left: R8 0xd4 subs 1 / 3 / 4 / 5 (option data) and
  the map preview, R9 0x20 / 0x30 control lock, R11 0xae, R12 0xab toggles, R13 0x7e mounts (needs the chocobo /
  mount model; subs 0 / 5 / 8 done), R15 0xb6 0x11 / 0x12..0x15 (0xd / 0xe chocobo looks done), R20 0xd8 body rotation, 0x46/3 camera seed, 0xb3 mini-game.
- New-character flow, one nation at a time (create, opening cutscenes, first zone):
  - San d'Oria: under review;
  - Windurst and Bastok: the openings looked right to the user (2026-10-23; Windurst Waters 531, Bastok Markets);
    left: Windurst's water (too dark; the world blends in gamma space since, MODLOG "The world scene blends in gamma
    space", not seen by the user yet; the lobby scenes still blend in linear space), the other openings (Port
    Windurst, Windurst Woods, Bastok Mines, Port Bastok) not seen yet.
- Other known gaps:
  - after the move to FFXiMain 30261006_0 (MODLOG "Tables copied from FFXiMain"): the creator backdrop drew black
    (on zone 133 too; not reproduced 2026-10-23: the lobby and the creator race step draw it live and
    offscreen); the gamepad rows (Misc. 4 0xce, the external gamepad
    tool);
  - magic and ability menus (done: lists, prompt, <st> tags, cast bar, mouse in the lists, the sort windows and the
    order files, cursor memory, the mount list, the emote list; left: blue magic sets (s2c 0x044), mouse in the
    prompt, the mount model under the rider, the Friend List);
  - the map (MODLOG "Map window: pan, zoom, fades, event maps"): done: pan, left-drag pan, zoom, fades, /map, event
    maps and views, markers and the .MRK file, Change Map, Wide Scan and tracking, the busy strip, the call-for-help
    pointer and `<ht>`, the compass radar dots (MODLOG "Compass radar dots"); left: helpwind's other busy bits;
  - items: "menu    iuse" Use and Drop, the direct-use list, itmsort2 Auto and Manual, the Recycle Bin (and
    `/recycle`, "menu    itemctrl") done; the storage views (text commands, both sides, itmsortw, itmstora,
    Alt+arrows) and the moogle menus (0x02E, myroom / mogext, storage menus, Change Jobs); left the Delivery Box and
    Remodel rows, the Mog Garden use rule; the Delivery Box done (send, receive, cancel mode);
  - NPC and guild shops (client/shop.rs, MODLOG "NPC shops", "Guild shops"): buy, sell, appraisal, the can-wear
    yellow, the requirement column, the scroll colours, the settled flag (s2c 0x01D) done; left the dice / attachment
    colours (need s2c 0x063 sub 4 and 0x044);
  - trading (client/trade.rs, MODLOG "NPC trade", "Player trade"): the NPC hand-over done and live-tested; player trade
    done but untested live (needs a second account); left mouse in both, the target prompt with no target;
  - /check on a player (client/inspect.rs, MODLOG "/check on a player"): the inspect window done; left the bazaar
    list behind View Wares (c2s 0x105);
  - main menu rows (specs in `~/ffxi/lab/research/`: `party_menu_spec.md`, `status_window_spec.md`,
    `quest_mission_spec.md`, `menu_rows2_spec.md` for Search / Linkshell / Region Info): Party done (client/partymenu.rs,
    levelsync.rs, MODLOG "Party menu", "Party menu: Member List, Level Sync, Focus, Rearrange"; left View Request; the
    invite side untested live);
    Status done (client/status.rs, currencies.rs, unity.rs, MODLOG "Status window", "Currencies", "Unity"; left Merit /
    Job / Alter Ego Points, spec `status_rows_spec.md`);
    Missions and Quests done (client/missions.rs, quests.rs, journal.rs, MODLOG "Missions", "Quests"; left the RoE
    lists); Key Items done (client/keyitems.rs, MODLOG "Key Items"); Search done (searchmenu.rs, arealist.rs,
    seacom.rs; left /seacom and /seacomup, arealist's event modes for op 0xcc 0x10 / 0x11); Linkshell (linkshell.rs:
    the item list, equip / unequip, naming, Drop, the message and /lsmes done, not seen with a real shell yet; left the
    Link List (link12) and member actions); Region Info done (region.rs; left the Campaign windows, which LSB never
    feeds); the social page (spec `social_rows_spec.md`): View House done (viewhouse.rs), Set Bazaar done
    (bazaar.rs, shop.rs, seacom.rs; left the name's bazaar glyph, View Wares); left Macros, Help Desk;
  - window groups (MODLOG "Window group walk"; spec `~/ffxi/lab/research/window_groups_spec.md`): FUN_1015fa00's walk
    over every open window by open order; left the raise tickets (a window re-raised while open), the main menu's
    own zoom;
  - combat mode (user, 2026-10-24: retail keeps the battle command menu, the top bar and the HP / MP / TP gauges up
    while engaged, and the camera cannot be spun): done (MODLOG "Engaged state", "Engaged menu", "Action menu: the
    mouse hold", "Chat mode window"); Sprint / Scout / Quarry show only in Conflict;
  - the treasure pool (client/treasure.rs, MODLOG "Treasure pool window"): loot / lootope / lnowin, lot, pass, Done,
    the party-window chest done and live-tested with a bot; left the zone-pool columns, the mouse;
  - skill actions (MODLOG "Skill actions"): holds, op 0x25 reactions, mdam messages, the death after the hits done;
    skillchains, knockback, guard / parry / block / counters from the files done (MODLOG "Skillchains", "Knockback",
    "Per-record reactions"); guards, parries and counters not seen live yet;
  - on-screen damage numbers (config 0x13; MODLOG "On-screen damage numbers"): done; left checking the glow copies'
    size against retail footage (the code ignores element scale), party / alliance relations for the colours;
  - status icons (spec `~/ffxi/lab/research/status_icons_spec.md`; MODLOG "Status icons"): own icons, timers, pulse,
    Num+ focus and cancel, party member icons (untested live: needs a party) done;
  - actor sounds (spec `~/ffxi/lab/research/actor_sounds_spec.md`; MODLOG "Actor sounds 1..3"): voices steal, init /
    ini<n> (worm emerge seen, burrow not seen live), ntob / bton / dead, op 0x78, op-0x24 swing pick, whole schedulers
    behind motion paths, sub-animation variants, op 0x53, pop0 / pop1 done; left the hide skip (+0x138), ?ead / sdep;
  - lobby (spec `~/ffxi/lab/research/select_buttons_spec.md`): title and list sounds and press animations, the
    prompt as retail, login screen sounds, the fade waits done; left the generic press rings on the Config pages;
  - UI sounds (audit `~/ffxi/lab/research/sound_audit.md`): done (the tell sound "0p40" is in no resident file; <ncall> not done);
  - the message formatter (dat/msgfmt.rs) and /clock done; left event codes 7f ab / ac, the other callers;
  - list scroll bars (MODLOG "List scroll bars"): done for the item, magic, shop, query, log, map and config lists;
    the mouse and the wheel on them; left the key item / quest / mission lists;
  - collision and lifts (MODLOG "Actor collision and lifts"): NPCs hold you 0.5 s then let you through, idle players never (MODLOG "Character bump"); lifts ride and move, the zone-in waits for them; left
    Mog House furniture boxes;
  - effects (fx/, render/effects.rs; MODLOG "Effect generators" and "Effect schedulers"): zone, weather, sprites,
    melee sparks, casting, spells, weapon skills, model generators, effect sounds, lens flares, D3b morphs, the
    moon, footsteps (sounds, dust, prints), heat haze, spline paths, the zone light tint and the Effects filter
    run, and point lights on scenery and actors; left: the party roster for the filter, moving-part lights;
    parked (user's call, 2026-10-22): replace baked lamp shells with real lights plus soft particles (MODLOG
    "Effect fixes from the San d'Oria gate lamps", "Parked");
  - OTP (TOTP) device enrolment for 2FA: net/auth.rs has CreateTotp / VerifyTotp / RemoveTotp, nothing uses them.
- Updates (user's design, 2026-10-22). Never required, always offered:
  - After login, character select gets an "Update" button above Select Character when anything is out of date
    (checked in the background at start). Pressing it opens a window asking what to update, each with its size:
    game files (src/patch.rs, the PlayOnline patch protocol, done as `vana-client update`), vana-client itself
    (GitHub releases), and later others (candidates: mod packs, the client version string LSB checks).
  - First start with no game files: download the retail installer from SE, unpack it, take what we need, then
    patch. While no DATs exist every other title button is disabled. Open question: the title and login screens
    are drawn from the DATs, so a first start needs some screen of its own before them.
  - No command line for any of it.
  - Releases (user, 2026-10-24): build the Linux binary with the Steam Runtime "sniper" toolchain so one build runs
    on every distro; the Update button downloads the release from GitHub, installs it and restarts the client.
  - The install was behind the server's data (NPC models 2957+ missing: the level-5 crystal scene's set was
    blank): 5321 files, 452 MB as of 30261006_0. Updates go to the base files, never to mods.
- Mod manager (XIPivot built in). Done: dat/overlay.rs. Mods are folders under ~/.local/share/vana-client/mods in
  XIPivot's layout (ROM*/<dir>/<file>.DAT, game-root files), ordered by config `mods` (highest first). An overlaid
  DAT is merged per chunk from the current stock file: D3s textures (by their own name), Msb sprite sheets and Bp
  bump maps from the highest mod, everything else stock; non-chunk files are replaced whole; merged builds cached
  in ~/.cache/vana-client/merged. Installed 2026-10-22 from ~/Downloads/FFXI by ~/ffxi/lab/tools/install_mods.py:
  Amelila upscale 02 / 01 / megapack 2019, xiPrime menus / status icons / cursors, Remapster, AshenbubsHD (lowest).
  Left: a settings page (enable / disable / order / add a pack from an archive); sound overlays (sound*/win) in the
  audio loader; root files (DatIndex::root_file) used for the cursors; mod VTABLE / FTABLE (new file ids, e.g.
  Remapster's 109 extra maps, which need an add-on in retail too).
- Extras (gameplay additions, the Windower / Ashita add-ons built in): the Config list's Extras row and page, all
  off by default, built from the Config pages' own sprites (MODLOG "Config: Extras and Settings"). Config-type
  options (resolution, frame limit, ...) go in Settings, not here. Done: the map overlay, the synthesis amount,
  timestamps on every line.
  - wanted: a minimap (after the map's pan and zoom);
  - candidates: target distance, party buffs / TP, target HP %, clock, recast timers, text auto-advance, Find
    across bags; later Battlemod-style filtering, a DPS meter, a skillchain helper.
- Chat log (user, 2026-10-23: "needs a lot of love"; audit `~/ffxi/lab/research/chat_audit/`): done the server side
  (treasure pool, MsgShift gaps, 0x017 kinds and echoes, 0x027 / 0x043, 0x009 keys, lines held until the message
  tables load, the server message and /servmes, zone text colours) and /emote, /recast, talk range / mounted, /party
  alone, Shift-JIS text both ways, auto-translate (dictionary, phrases, Tab completion), /clock, the /l linkshell
  check (s2c 0x0E0), /playtime, /ver, /chatmode, the on / off toggles, /localsettings, the line history (Up / Down)
  (`~/ffxi/lab/research/textcmds2_spec.md` parts A and B). Left: invisible talk, /equip refusals, the command gate's
  lines (window open, layout, charmed), the map on an elevator, the block list, s2c 0x0B4 (PvP, the system message
  filter); the toggles' consumers for /ignorepet, /ignoretrust, /hidetrust (no pet owner / trust flag on entities),
  /battlebgm (party battle music), /bellsw (op 0xb3), /furcc; /displayhead, /partyinfo, /jobmasterdisp, /autotarget; the spec's part C (window, list and server-flag commands).
- Last of all (the user's plans, 2026-10-23): a modern UI as an Extras option, keeping the classic feel: windows
  that can be moved, elements turned on and off, more than new art; after that, an embedded Lua layer for mods.
- Portability to Windows and macOS: see "Portability notes" in MODLOG. Windows (MinGW cross build, tested by the
  user in WinBoat) left: the exe icon (a resource), a fully static link (it still imports libwinpthread-1.dll).
