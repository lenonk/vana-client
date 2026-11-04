## Event ops 0xa8 / 0xb9: map marker clear reaches the map (2026-11-02)

- Ops 0xa8 (FUN_100b9780) and 0xb9 (FUN_100b9820) have fixed lengths (6 / 8), but byte 1 picks the form: sub 0
  loads the zone's marker file (FUN_100b2570(4, zone, key); 0xa8 passes key -1), clears slot val@4 / val@6
  (FUN_101f8da0) and its name (FUN_101f8e30 ""), then saves (FUN_100b2a60); sub 1 clears the loaded data only; 2 and up
  do nothing. Retail, current build 30261006_0 (dispatcher jump table 0x100bc960).
- Ours had the map side (`map::Cmd::Unmark`) since "Event marker ops", but the op table gave both ops a fixed length,
  so the VM never read the sub byte: the host got no sub and no operands and the clear never ran (304 sites in 32 zones
  of 0..299). The table now reads the form from byte 1 with the same fixed lengths.
- Event sweep of zones 43 / 50 / 101 / 102 / 103 / 208 / 238 / 257 unchanged (3,090 events, 0 faults). Test:
  `marker_clear_subs`. Not checked live.
