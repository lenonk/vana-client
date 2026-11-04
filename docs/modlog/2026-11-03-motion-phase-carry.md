## Motions: a new one starts at the old one's phase (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch5.md item 9; MODLOG's FUN_10019ed0 is an older
  build, this build's enqueue is FUN_1001ad00): when a motion is queued into a region that already holds one, its
  start frame is not 0. With lengths in frames / speed:
  - k = 1 / oldLength, pos = (old entry's current position) x k x newLength + k x newLength x newSpeed x 0.5 x rate
    x blend, taken modulo newLength;
  - start frame = newSpeed x pos.
  Then the old entry starts its fade-out. So a walk becoming a run (or idle becoming walk) picks up at the same
  fraction of the cycle, pushed on by half the blend, and the feet stay in step.
- Ours: client/mod.rs sets Actor::ticks from the previous motion's clock by that formula instead of 0 (our tick is
  twice retail's frames / speed unit: a frame is ticks x speed x 0.5). A one-shot still starts at 0.
- Not built: retail does this per region. Ours has one clock for the actor's status motion, so a motion that keys
  only some regions carries the same phase into all of them.
