## Event model filter: doors, lifts, ships and reserved actors keep their model (2026-11-03)

- Retail (docs/re/notes/2026-11-03-modlog-gaps-batch5-build.md §2): FUN_1008e4d0 only marks an actor for unloading
  (+0x120 bit 23); FUN_10095900 acts on the mark. It never unloads an actor that is bound, takes part or is held
  (FUN_100961d0, +0xf2), nor a door, elevator or ship (+0xee 3..5, FUN_10096200), nor a client-made local actor
  (+0x130 bit 3). The event's filter of non-participants ((DAT_104808fc | Mode) & 0x02 for PCs, & 0x10 for the rest)
  also skips an event-reserved actor (+0x138 bit 31, op 0xac sub 2); /hidetrust's mark does not.
- Ours (`event::hidden`, which the model and the name plate both follow) took doors, elevators, ships and reserved
  actors out with the filter. Now `unloads` applies both exemptions. The server hide bit (0x00E @0x20 bit 1) still hides
  them, as retail's separate hide. Test `unload_exemptions`.
- Not done: the hold count +0xf2 (FUN_100833b0 / FUN_100833c0, callers not traced) and the +0x1ff status rule (P+0x130
  bit 11 with statuses 2..6 / 40..43).
