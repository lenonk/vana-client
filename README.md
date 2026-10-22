# vana-client

A native client for FINAL FANTASY XI, written in Rust, for [LandSandBoat](https://github.com/LandSandBoat/server)
(LSB) servers. It runs without Wine and without PlayOnline: it reads the game's own data files and talks to the
server directly.

The goal is a faithful clone of the retail client first. Its menus, sprites, fonts, motions and cameras are rebuilt
from the game's own files and from the behaviour of the original executable. Improvements come after that.
Retail limits that only existed because of 2002 hardware, like the 30 fps cap, become settings.

Linux is the current platform. Windows and macOS are planned.

## Status

Early, but playable on a local LSB server. Working today:

- Login, the lobby, character creation (with the original creator scenes) and character select
- Zones with GPU skinning, compressed textures with mipmaps, lighting, sky, fog and weather
- Players, NPCs and monsters with their motions, name plates and ground collision
- NPC dialogue, event scripts and cutscenes
- The HUD, chat log and input line, targeting and lock-on, combat and the action menu
- Items, equipment, the map window, and the in-game and lobby Config menus
- Music and sound effects

Not done yet:

- The full retail key set: numpad movement, autorun, first person, F-key targeting, macros and palettes
- The magic, ability, weapon skill and pet menus
- Spell and ability effects
- Collision with characters and moving platforms
- A patcher for game updates
- Windows and macOS builds

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

The binary is `target/release/vana-client`.

## Install the game data

Point the installer command at the first part of the official setup, or at an extracted copy of it:

```sh
target/release/vana-client install /path/to/FFXIFullSetup_US.part1.exe
```

This unpacks the game to `~/.local/share/vana-client/game/FINAL FANTASY XI` with the same layout the Windows
installer uses. Use `--game-dir` to put it somewhere else. An existing Windows install can also be copied there,
or pointed at with `game_dir` in the config (see below).

## Play

```sh
target/release/vana-client play
```

You log in on the game's own login screen. The server address and ports come from the config file.

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
