<p align="center"><img src="assets/vana-client.png" alt="vana-client icon" width="160"></p>

# vana-client

A native client for FINAL FANTASY XI, written in Rust, for [LandSandBoat](https://github.com/LandSandBoat/server)
(LSB) servers. It runs without Wine and without PlayOnline: it reads the game's own data files and talks to the
server directly.

The goal is a faithful clone of the retail client first. Its menus, sprites, fonts, motions and cameras are rebuilt
from the game's own files and from the behaviour of the original executable. Improvements come after that.
Retail limits that only existed because of 2002 hardware, like the 30 fps cap, become settings.

Linux is the current platform. Windows and macOS are planned.

## Status

Playable on a local LSB server. Working today:

- Login, the lobby, character creation (with the original creator scenes), character select and the new-character
  openings
- Zones with GPU skinning, compressed textures with mipmaps, lighting, sky, fog and weather
- Players, NPCs and monsters with their motions, collision (characters, doors, lifts) and name plates with their
  icons (GM, bazaar, New Adventurer, mentor, linkshell and the rest)
- NPC dialogue, event scripts and cutscenes
- Effects: spells, abilities, weapon skills and skillchains, damage numbers, footsteps, level up and the other
  milestone effects
- The retail key set: numpad movement, autorun, first person, F-key targeting, macros and palettes
- The HUD, chat log and chat line (chat modes, auto-translate, text commands), targeting, combat, and the magic,
  ability and mount menus
- The main menu: Status (merit points, job points, currencies, Unity), Equipment and equipment sets, Items and
  storage, the map, Party and Level Sync, Linkshell, Search, Missions, Quests, Key Items, Region Info, Bazaar, the
  Help Desk and the macro editor
- Trading, NPC and guild shops, the Delivery Box and the treasure pool
- The in-game and lobby Config menus, with an Extras page for additions such as a map overlay and chat timestamps
- Game updates from the PlayOnline patch server, and HD texture packs through a built-in mod overlay
- Music and sound effects

Not done yet:

- Alter Ego Points, automaton customization, blue magic sets, the mount under its rider and the Friend List
- Some event script operations and chat log details (the block list among them)
- An in-client update button, a settings page for mods, a minimap
- macOS builds; Windows builds work through a MinGW cross build

## Requirements

- A GPU with Vulkan support
- Rust 1.85 or newer (edition 2024)
- PipeWire or PulseAudio for sound
- The game data from Square Enix's official installer, `FFXIFullSetup_US.part1.exe` and its other parts
- A LandSandBoat server to connect to

No game data is included in this repository.

## Build

```sh
cargo build --release
```

The binary is `target/release/vana-client`. Release builds are incremental, without link-time optimisation; for a binary to hand out, `cargo build --profile dist` (thin LTO) writes `target/dist/vana-client`.

## Install the game data

Point the installer command at the first part of the official setup, or at an extracted copy of it:

```sh
target/release/vana-client install /path/to/FFXIFullSetup_US.part1.exe
```

This unpacks the game to `~/.local/share/vana-client/game/FINAL FANTASY XI` with the same layout the Windows
installer uses. Use `--game-dir` to put it somewhere else. An existing Windows install can also be copied there,
or pointed at with `game_dir` in the config (see below).

The installer's data is years old. To bring it up to the current retail version from the PlayOnline patch server:

```sh
target/release/vana-client update
```

## Play

```sh
target/release/vana-client play
```

You log in on the game's own login screen. The server address and ports come from the config file.

To add it to your desktop's application menu with its icon, put the binary on your `PATH` and install the desktop
entry. Wayland compositors find the icon through this entry rather than through the window:

```sh
install -Dm755 target/release/vana-client ~/.local/bin/vana-client
install -Dm644 assets/vana-client.desktop ~/.local/share/applications/vana-client.desktop
install -Dm644 assets/vana-client-512.png ~/.local/share/icons/hicolor/512x512/apps/vana-client.png
```

To create an account on the server from the command line:

```sh
target/release/vana-client account-create --credentials account.json --user NAME
```

This writes the username and a generated password to `account.json`.

## Configuration

Settings are in `~/.config/vana-client/config.json`. The file is created on first save, and most options can be
changed from the in-game Config menus. The fields you may need to set by hand:

| Field | Default | Meaning |
|---|---|---|
| `server` | `127.0.0.1` | LSB server address |
| `auth_port`, `data_port`, `view_port` | `54231`, `54230`, `54001` | LSB login and lobby ports |
| `game_dir` | `~/.local/share/vana-client/game` | Folder that holds `FINAL FANTASY XI` |
| `client_version` | `30261006_0` | Version sent to the lobby; must be at least the server's `CLIENT_VER` |
| `frame_limit` | `300` | Frame limit in fps, `0` for unlimited |
| `gpu` | empty | Graphics adapter by name; empty picks the high-performance one |
| `audio_device` | empty | Output device; empty uses the system default |

## For developers

- `MODLOG.md` records every reverse-engineered fact the code relies on: addresses in the original executable,
  DAT formats, packets. Anything inferred rather than read from the binary is marked as inferred. Read the
  relevant section before changing behaviour, and add what you learn.
- `cargo test --release` runs the tests. Tests that need the game data skip themselves when it isn't installed.
- Offscreen tools render parts of the client to PNG without opening a window, for example `zone-shot`, `npc-shot`,
  `char-shot`, `select-shot`, `creator-shot` and `cs-shot`. Others decode game files: `dat-info`, `ui-dump`,
  `event-dump`, `msg-dump`. Run `vana-client --help` for the full list.
- `play --input '<script>'` drives the client from a script for test runs (see `client/script.rs`).

## Disclaimer

This is an unofficial fan project. It is not affiliated with, endorsed by or supported by Square Enix.
FINAL FANTASY is a registered trademark of Square Enix Holdings Co., Ltd. The repository contains no game data and
no code from the original client. To use it you need the game files from Square Enix's installer.

## License

No license has been chosen yet.
