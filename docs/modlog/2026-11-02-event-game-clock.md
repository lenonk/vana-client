## Event op 0x83: the game clock (2026-11-02)

- Op 0x83 (FUN_100b90e0, current build 30261006_0): var@1 = FUN_101039e0, the game clock in Earth seconds since the
  Vana'diel epoch (FUN_100dc130's whole seconds plus the server offset DAT_104e0af8), 3 bytes. Ours left the variable
  untouched, so scripts read 0 or an old value (285 sites in 98 zones of 0..299).
- Scripts divide it by 3456 (a Vana'diel day) or 103680 (30 days) for cycles, e.g. Port Bastok (231) counts days
  left in a 25-day cycle before message 7022.
- Ours: Host::game_time, LiveHost from `vana_now` (s2c 0x00A's time plus the time since). Test: `game_clock_83`.
  Event sweep of zones 2 / 4 / 5 / 7 / 24 / 231 / 232 unchanged (1,307 events, 0 faults). Not checked live.
