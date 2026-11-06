## Effect elements: no t = 0 frame (2026-11-05)

Heavens Tower's waterfalls (sub-area 426, `r_5w/effe/wat0/tpB1` / `tpB2`, mesh `tk00`) flashed at full strength for one
frame every respawn (life 60, a new sheet every ~22 ticks).

- The sheets' alpha key `tk01` is (0, base), (1e-7, 0), (0.49, 0.22), (0.9999999, 0), (1, 1): the first and last points
  are guards that a sample in (0, 1) never reaches. Op 0x2d's base is the element's colour alpha (0x80), so a sample at
  exactly t = 0 shows the sheet at alpha 0x80 instead of ~0.
- Ours spawned the element with LIFE_SET (the CPU's lives ran before the spawn, so the GPU skips its first decrement
  to stay in step); its first update then sampled t = 0. Retail counts the life down before the keys, so a timed
  element's samples run t = dt / life .. (life - 1) / life *(inf: from the guard points; no DLL pass)*.
- Fix: a spawn also sets ctl::BORN; the update pass runs the element's stream as before but leaves it undrawn that
  frame when it has a life and lives on past the next decrement (outs[23] = 0). Elements without a life (t stays 0)
  and one-tick elements (life - dt < 1: their only frame) draw at once. Point lights and op 0x60 flashes are not
  gated (they do not read outs[23]). Test:
  `gpu_born_hidden_one_frame`.
