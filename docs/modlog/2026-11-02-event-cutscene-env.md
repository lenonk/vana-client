## Event ops: the cutscene clock, sub-area wait and wind gusts (2026-11-02)

- Op 0xa9 (FUN_100bab50, from the dispatcher table at 0x100bc960) holds the Vana'diel clock still (FUN_1018c4f0) at
  day val@1 x 7, 00:30 (FUN_1018be70 / bfa0 / bf00; one minute = 3600 units, a day 0x4f1a00) and sets
  DAT_1065cb20, so the weekday (FUN_1018bd90 = day & 7) and moon phase (FUN_1018bde0 = ((t + 12 h) day / 7) % 12)
  come from that clock instead of the server's. Moon phase = val % 12. The zone lighting, the music hour and the
  effect generators' weekday / moon ops (FUN_1004a980) read it while the event runs.
- Op 0xc9 (FUN_100b87e0): if the clock is held (DAT_104886da, set by 0x77 with an hour or by 0xa9), it runs again
  (FUN_1018c4d0) and DAT_1065cb20 clears. Op 0x78 (FUN_100b87b0) does the same and goes back to the server's
  weather (FUN_100940f0). Before this, 0xc9 did nothing, and 0x78 was the only way back.
- Op 0x77 (FUN_100b8710) with an hour holds the clock's current day at that hour, minute 0 (FUN_1018bf50 / bf00);
  it keeps a day and the moon source that 0xa9 set. Event state keeps the held clock and 0x77's weather apart
  (`HeldClock` in client/event.rs); both still end with the event as before (retail only restores through
  0x78 / 0xc9; FUN_100aed00 just clears DAT_104886da).
- Op 0x98 (FUN_100b8630) ends the frame while a sub-area load runs (DAT_10631f24 +0x44), the same check as
  op 0x75 sub 0 (`SubArea.loading` / `asked`).
- Op 0x97 (FUN_100ba0c0) sets the wind gust range of the environment object (DAT_1065cb04 +0x138 base / +0x13c
  spread, stock 4 / 10 from FUN_10187050; saved once by FUN_100ba040). FUN_10188a50 toggles gusts at random
  (rand % 60 frames, x1.2 while blowing) with strength (rand % spread + base) x 0.0025, decaying 0.01 a frame
  when calm, and FUN_10023700 pulls the cloth vertices of character meshes toward +0x124 by it. Our client draws
  cloth vertices skinned and has no cloth simulation, so 0x97 stays unhandled until one exists.
- Op names and descriptions for 0x77 / 0x78 / 0x97 / 0x98 / 0xa9 / 0xc9 in dat/event_ops.rs now name the real
  handlers (the old FUN names were off); 0xc9 is `clock_run`.
