# XIV-UI (suite mod)

One mod bundles eight compact modern HUD panels (target, focus, cast, player, buff, experience, party, notify) behind per-panel **`mod_<panel>`** toggles in a single `mod.toml`.

The catalogue lists **`xiv-ui` only** — not separate per-panel mod ids. All panel code ships under `modules/<panel>/` inside this tree.

## Using the suite

1. Open **XIV-UI** in the Mods window.
2. Turn on **`mod_<panel>`** for each panel you want, then **`/lua reload xiv-ui`** if that panel was off at the last reload.
3. Turning a panel off hides it without reload; turning one on after it was off at load still needs reload.

## Saves and layout

- Panel positions and non-manifest options live in **`storage.account.xiv_ui_<panel>_v1`** buckets for this mod only.
- Layout keys (`offset_x`, `place_x`, …) are **not** in suite `mod.toml`; they live in each panel’s `config_schema.lua` bucket (`xiv_ui_<panel>_v1`) and `xiv_config` when exposed there.
- **Notify:** toast position uses that layout bucket; invite reminder offsets (`notify_request_offset_*`), scale, seconds, and toggles are **`notify_*` keys in `mod.toml`** only.

## Developer layout

| Path | Role |
|------|------|
| `init.lua` | Suite loader, frame pump, settings toggle hints |
| `mod.toml` | **Only** manifest (permissions, settings, commands) |
| `modules/<panel>/entry.lua` | Panel runtime (always read from disk) |
| `modules/<panel>/manifest.lua` | `source_files` import list per panel |
| `modules/<panel>/src/` | Panel Lua loaded at runtime |
| `img/` | Retained PNGs only: `jobs/` (22); vitals, notify, and mog indicator use `hud` primitives |
| `src/` | Shared bridge, bootstrap, `xiv_renderer` (`src.xiv` per panel), `hud_config_factory`, `hud_drag` |
| `tests/` | Mod VM tests run by `cargo test -p vana-client mod_tests` |

Panel **`entry.lua`** files are the runtime entry points; there is no separate `modules/*/init.lua` in the suite.

## Native HUD coverage

With the matching `mod_*` toggles on, panels hide or redraw the usual combat HUD pieces: **target** (incl. status/debuff strip), **focus/last**, **cast**, **party/alliance**, **your buff row**, and the **experience** strip. **Player bars** draw custom vitals over the hotbar area. **Notify** adds toasts; it does not replace a large native window.

The client **Timers** edge window (especially **recast** lists) is **not** replaced — keep Timers or another mod if you want recasts.

## Attributions

### vana-client (this repository, MIT)

Reused or aligned with other **vana-client** catalogue assets — not third-party.

| What | Where in xiv-ui | vana-client source |
|------|-----------------|-------------------|
| Job badge PNGs | `img/jobs/*.png` | [`assets/modern/svg/jobs/*.svg`](../../modern/svg/jobs/) — see [`img/jobs/README.md`](img/jobs/README.md) |
| EXP / merit / JP gain math | `modules/exp/src/points.lua` | [`assets/mods/xp-tracker/init.lua`](../../mods/xp-tracker/init.lua) (v1.0.2) |
| Player HP/MP/TP strip | `modules/player/` (`hud.bar`) | Same client **`hud.bar`** pattern as [`assets/mods/player-bars/init.lua`](../../mods/player-bars/init.lua); layout/UI differ |
| Status land / expire heuristics | `modules/target/src/status_observer.lua` | Battle message ids aligned with [`assets/mods/target-debuffs/init.lua`](../../mods/target-debuffs/init.lua); logic is suite-specific |
| Action → status fact tables | `modules/target/src/budget_data/*.lua` | Compiled from **vana-client** game/action data (in-repo export; sharded for M18 budget) |
| Trust job badges (party) | `modules/party/src/trusts.lua` | **`vana-client trust-gen`** from LandSandBoat SQL (see below); not third-party mod code |
| Bars, icons, chrome | all panels | Client **`hud.bar`**, **`hud.icon`**, **`hud.rect`** (no shipped bar/status PNG dumps) |

### Generated (LandSandBoat SQL)

- **Party trust job hints** (`modules/party/src/trusts.lua`): generated with `vana-client trust-gen --lsb <checkout>` from LandSandBoat `spell_list` + `mob_pools` (trust `poolid = spellid + 5000`, same join as retail/LSB). Regenerate when trusts are added upstream.

## Manual check before merge

- `/lua reload xiv-ui` with several `mod_*` panels enabled — no `no setting … in mod.toml` errors.
- Drag player / target; party cast strips with target on; buff click-cancel; XP attach to player bars when enabled.
- `VANA_MOD_TEST=xiv-ui cargo test -p vana-client mod_tests`