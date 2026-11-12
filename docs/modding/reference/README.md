# Reference

Every function of the mod API, one page per namespace. Each function has its signature, a table of its parameters
(type, whether it is required, the default), what it returns, the errors it raises, the permission it needs, and an
example. Tables that several functions share (a player, an entity, an item, a window's spec and so on) are on
[types.md](types.md).

New to modding? Start with [getting started](../getting-started.md) and [the guide](../guide.md).

## Reading signatures

```lua
game.entity(id) -> Entity?
actions.cast(spell, target?) -> nil
```

- A `?` after a parameter means it can be left out; the parameter table gives the default.
- `-> Entity?` means the function returns an [Entity](types.md#entity) table, or `nil`.
- `-> nil` or "Returns nothing" means there is no return value.
- Where a function returns several values, they are listed in order: `-> boolean, string?`.

## The basics

| Page | What it covers |
| --- | --- |
| [Globals](globals.md) | `print`, `require`, `sleep`, `sleep_frames`, `wait_until` and the `mod` table. |
| [events](events.md) | Handlers for what happens in the game (zone changes, HP, chat, actions, ...), and the full list of events with their arguments. |
| [commands](commands.md) | Your mod's chat commands, and running lines as if the player typed them. |
| [timer](timer.md) | Calls after a delay or at an interval. |
| [settings](settings.md) | Options declared in mod.toml, shown on the mod's Extras page. |
| [storage](storage.md) | Data saved for you between sessions, per account and per character. |
| [log](log.md) | Lines in the client's log file. |
| [Types](types.md) | The shared tables: Player, Entity, Member, Item, Action, WindowSpec, Packet, KeyEvent, MouseEvent, Time, Mod. |

## Reading the game

| Page | What it covers |
| --- | --- |
| [game](game.md) | Your character, the zone, entities, party, bags, equipment, buffs, recasts, jobs, the clock, maps, collision. |
| [res](res.md) | The game's fixed data: items, spells, abilities, status effects, zones, titles, key items, augments, messages, auto-translate entries, jobs, weather, game files. |
| [text](text.md) | The game's text: Shift-JIS bytes, text without its control codes, auto-translate phrases. |

## Acting in the game

| Page | What it covers |
| --- | --- |
| [actions](actions.md) | Cast, use abilities and items, equip, target, move, talk to NPCs, trade, buy and sell, party commands. Needs `actions` or `automation`. |
| [binds](binds.md) | Key combinations tied to a text line or a function. |
| [input](input.md) | The keyboard and mouse before the game sees them; pressing keys for the player. |

## Showing things

| Page | What it covers |
| --- | --- |
| [chat](chat.md) | Lines in the chat log, and the chat line. |
| [filters](filters.md) | Keep, change or drop chat lines and typed lines. |
| [hud](hud.md) | Text, boxes, bars, images and icons over the game. |
| [ui](ui.md) | Your mod's windows: lists, buttons, sliders, inputs, canvases; prompts and menus. |
| [ui.native](ui.native.md) | The game's windows and HUD: what is open, the highlighted row, hiding, moving and scaling panels, where they are, item colours, the game's config values. |
| [ui.skin](ui.skin.md) | The modern UI's look: its sprites, colours and window frames. |
| [draw](draw.md) | Lines, rings, boxes, labels and pictures in the 3D world. |
| [render](render.md) | How entities look, post-process shader passes, the camera. |
| [timers](timers.md) | Rows in the client's Timers window. |
| [audio](audio.md) | Your mod's WAV files and the game's sounds. |

## Files, network and other programs

| Page | What it covers |
| --- | --- |
| [fs](fs.md) | Files in your mod's data folder. |
| [json](json.md) | Lua data to JSON text and back. |
| [net](net.md) | HTTP requests, TCP, UDP, local sockets; opening a web page. |
| [packets](packets.md) | Watching, changing, blocking and sending game packets. |
| [devices](devices.md) | USB HID devices by their raw reports. |
| [client](client.md) | The client program: clipboard, behaviour switches, the window, screenshots, recording, running programs, process priority and CPU cores. |

## Mods together

| Page | What it covers |
| --- | --- |
| [mods](mods.md) | The other mods: functions one mod offers the others, events between mods. |
| [ipc](ipc.md) | Messages between copies of your mod in several game windows. |
| [lobby](lobby.md) | The screens before login, for mods with `lobby = true`. |

## Permissions

Reading the game, drawing and your windows need no permission. Everything that acts in the game or reaches
outside it does; the player grants it on the mod's Extras page. Each function's **Needs** line names its permission,
and [the guide's permissions section](../guide.md#permissions) lists them all.
