## Name plates: scheduler op 0xb4's lift, and none on luopans (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch12.md item 80): scheduler op 0xb4 (0x1005c81d ->
  0x10062180) starts a task that sets the scheduler actor's +0x8c0 to the op's f32 @8 for s16 @6 x speed ticks. When
  it ends (0x10062140), +0x8c0 goes back to 0. The plate projection FUN_100d04c0 adds +0x8c0 to the anchor's y
  (0x100d04e3). Y is down, so the 8 `cst0` schedulers' -1.5 for 600 ticks raise the plate. The op does not wait.
- Ours: fx/tasks.rs queues `Effects::plate_lifts`; client/mod.rs keeps `Actor::plate_lift` and counts it down by
  frame ticks; `plate_point` adds it.
- Item 79, the plate gates (FUN_100d0510):
  - The chocobo / mount / rod / chair actors (+0xa4 1..4, 0x54, ...) have no plate. Ours draws plates only for
    entities, and those actors are not entities, so this already held.
  - Models 0xb22..0xb31 (the luopans, FUN_10098320, the same test petrow.rs uses for TP) have no plate. They now
    have none.
- Not built:
  - Ending the scheduler's actor link ends the task early (0x10062100). Ours runs the lift to its last tick.
  - 0x100d0710 reads +0x8c0 too *(inf: the arrow's twin)*; ours lifts only the plate.
