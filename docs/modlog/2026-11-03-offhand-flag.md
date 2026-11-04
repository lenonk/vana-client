## Dual wield: the off-hand flag as scheduler value 0x3b (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch5.md item 8): the swing pick FUN_100d6850 calls
  vt+0x250 on every swing: 1 when the model is dual wielding (+0x8b5) and the chosen scheduler's name starts with
  'b', else 0. That writes drawobj +0x840 bit 6.
- Its only reader is FUN_10062900, the scheduler system-value reader, as value 0x3b (0x3a is the sub-animation,
  0x3c the entity's +0x124 bits 8..11). So a scheduler in the swing's own files can branch on "this swing is the
  off-hand one", e.g. to put a trail on the other hand. The flag stays until the next swing clears it.
- Ours: Actor::off_hand is set at each swing in client/combat.rs, and fx/tasks.rs answers system value 0x3b from it
  through the new Actors::off_hand hook.
- Which scheduler files test 0x3b was not scanned, so nothing visible is expected to change yet.
