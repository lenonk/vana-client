# vana-client

A native client for FINAL FANTASY XI, in Rust, that logs into a LandSandBoat (LSB) server. No Wine, no PlayOnline.
Rendering is wgpu (Vulkan), windows winit, audio cpal (PipeWire), UI the game's own menus and sprites (egui only for
input and the F12 debug overlay). Faithful to retail first; improvements come after.

## Where things are
- Code: `crates/vana-client/src` (client/ game logic and UI, render/ GPU, dat/ game file formats, net/ protocol,
  audio/).
- `MODLOG.md`: every reverse-engineered fact (FFXiMain addresses, DAT formats, packets), plus what's inferred.
  Read the relevant sections before changing anything, and append what you learn.
- Game install: `~/ffxi/game/FINAL FANTASY XI`. AshenbubsHD and Remapster DAT packs are installed; the originals
  and a restore script are in `~/ffxi/dat-backup/`.
- FFXiMain decompile: `~/ffxi/lab/creatorwork/ghidra/all.c` and `all.asm`; unpacked DLL:
  `~/ffxi/lab/compress_re/unpacked/FFXiMain_unpacked.dll`. These stay outside the repo; never commit them.
- Research specs for upcoming work: `~/ffxi/lab/research/`. `keys_spec.md` covers all 180 key functions.
- Helper scripts: `~/ffxi/lab/tools/` (`ptrfind.py` VA/offset mapping, `keymap.py`, `mkcnf.py`, `append_modlog.py`,
  `delchar.py NAME --account LOGIN [--yes]` deletes a character's every DB row so the name can be remade; dry run
  without `--yes`, refuses while the character has an open session).
- LSB server: `~/landsandboat`. To restart a crashed map server, run from there:
  `setsid -f ./xi_map --log log/map-server-54230.log --ip 127.0.0.1 --port 54230 > /dev/null 2>&1 < /dev/null`

## Build and test
- `nice -n 19 cargo build --release`, `nice -n 19 cargo test --release`. The user runs
  `target/release/vana-client play`.
- Offscreen tools for checking without a window: `npc-shot`, `char-shot`, `select-shot`, `creator-shot`,
  `zone-shot`, `topbar-shot`, `cs-shot`, `flash-shot` and others (`--help`).
- Live test runs: `play --credentials ~/ffxi/lab/lnxtest01.json --input '<script>'` (steps in client/script.rs).
  Driven windows are titled "[test]" and ignore real input.

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
  - For DB reads, use a 0600 defaults file (`~/ffxi/lab/tools/mkcnf.py`) and delete it afterwards.
- Every live world run ends with a clean logout (`--quit`), with a timeout long enough for the 30 s logout.
  - Never kill a logged-in client: stale sessions cause "already logged in".
  - Stop on that message; don't retry in a loop.
- One test window at a time, killed by PID only, never pkill. Check `ps` first: the user may be playing in their
  own window. Use a config copy (`FFXI_CONFIG`) and audio off (`FFXI_AUDIO_DEVICE=alsa:null`) for test runs.
- Before profiling, check that the CPU and the GPU (an eGPU over Thunderbolt) are otherwise idle.

## Next up
- Finish the retail key set from `~/ffxi/lab/research/keys_spec.md`. Done: the input layer (client/input.rs),
  movement keys and the retail free camera (client/camera.rs). Left:
  - targeting: F1-F12, Ctrl/Alt+F8, screen-order Tab / Shift+Tab, Num0, Enter with no target;
  - UI hide, screenshots, the Pause log-out prompt;
  - macros and palettes.
- New-character flow, one nation at a time (create, opening cutscenes, first zone):
  - San d'Oria: under review;
  - Windurst: both opening cutscenes after zoning in are badly wrong (seen 2026-10-22 with a new Mithra MNK; the
    first, event 0x131 in Port Windurst, ran to its end);
  - Bastok: not reviewed yet.
- Other known gaps:
  - magic and ability menus (Ctrl+M/J/W/B);
  - the map's pan, zoom and its actions;
  - collision with characters and moving platforms;
  - effects (fx/, render/effects.rs; MODLOG "Effect generators" and "Effect schedulers"): zone, weather, sprites,
    melee sparks, casting, spells, weapon skills, model generators, effect sounds, lens flares, D3b morphs, the
    moon, footsteps (sounds, dust, prints) heat haze, spline paths and the zone light tint run; left: point lights, the
    config 0x92 effect filter;
  - random names in the creator;
  - OTP (TOTP) device enrolment for 2FA: net/auth.rs has CreateTotp / VerifyTotp / RemoveTotp, nothing uses them.
- Portability to Windows and macOS: see "Portability notes" in MODLOG.
