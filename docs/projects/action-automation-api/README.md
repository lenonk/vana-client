# Action automation API

Focused design for client Lua APIs that replace per-mod packet matching when chaining spells, abilities, weapon skills, and items.

**Motivation:** [command-script](../../../crates/vana-client/assets/mods/command-script/) (`match.lua`, `runner.lua`) duplicates logic from [pending-actions](../../../crates/vana-client/assets/mods/pending-actions/). Tier 1 APIs should expose the same facts from Rust so mods can `await` completion instead of listening to `0x028`.

## Cursor goal

Track implementation in the chat goal: **Action automation API (game.action_ready, game.pending, actions.when_done)**.

## Tier 1 (target first PR)

| API | Shape (sketch) | Meaning |
|-----|----------------|---------|
| `game.action_ready()` | `boolean` | Safe to send another player command (no cast bar / event bar blocking, optional: not in menu lock). |
| `game.pending()` | `table?` | Current outstanding ask, if any: `{ kind, id, target, state, since }` aligned with pending-actions. |
| `actions.when_done()` | callback / promise | Fires once when `pending()` resolves (answered, refused, or timeout). |

**Acceptance:** command-script `then` lines can use `when_done` + `action_ready` instead of `packets.on` + `match.lua` for spell/JA/WS/item chains (buff lines may still use `buff_gain`).

## Tier 2

- `actions.cast` / `use` return a handle with `:wait(timeout?)`.
- Normalized `events.on("action_start")` / `action_finish` with `kind`, `id`, `target` (optional; today mods parse `packets.on`).

## Tier 3

- Document `game.cast()` vs animation-only bars (`cast_event`, instant spells).
- Optional `events.on("buff_gain", { spell_id = … })` for scripted buff confirmation.

## Rust touchpoints

| Area | File | Notes |
|------|------|-------|
| Cast bar | `crates/vana-client/src/client/lua/state.rs` (`game.cast`) | `World.cast_bar`, `cast_spell`, `cast_event` |
| Combat / magic | `client/magic.rs`, `client/combat.rs` | Open/clear cast bar |
| Pending logic (reference) | `assets/mods/pending-actions/init.lua` | Move core state machine to Rust or shared Lua module |
| Actions Lua | `client/lua/` (actions module) | `when_done`, readiness |
| Stubs / docs | `assets/mods/library/vana.lua`, `docs/modding/reference/game.md` | Keep stubs in sync |

## Implementation plan

1. **Extract** pending-actions state into `PendingAction` on `World` (updated from existing packet handlers in client, not only in a mod).
2. **Expose** `game.pending()` and `game.action_ready()` from `state.rs`.
3. **Add** `actions.when_done(fn)` (or `tasks.defer` pattern) in the Lua runtime; document timeout semantics.
4. **Refactor** command-script behind a mod setting `use_client_pending = true` for gradual rollout.
5. **Tests:** Rust unit tests for pending transitions; mod tests under `command-script/tests/` with mocked pending API.

## Non-goals (this track)

- Changing server packets or LandSandBoat behaviour.
- Catalogue / pending-actions UI redesign (chip can keep using the same core).

## Status (Tier 1)

Implemented in the client (`crates/vana-client/src/client/pending.rs`):

- `game.action_ready()` — no outstanding ask, cast bar, event timer, or cutscene event.
- `game.pending()` — outstanding ask from c2s 0x01A / 0x037 (same rules as pending-actions).
- `actions.when_done(fn)` — one-shot callback when the ask resolves.

**Command Script 1.0.16:** setting `use_client_pending` (default on) chains spells/JAs/WS via `game.pending` / `game.action_ready`; self-buffs still use `buff_gain`. Packet hooks stay available when the setting is off or the APIs are missing (mod tests).

## Branches

- `feat/command-script` — Command Script mod (shipped).
- `feat/action-automation-api` — API design + Tier 1 implementation.