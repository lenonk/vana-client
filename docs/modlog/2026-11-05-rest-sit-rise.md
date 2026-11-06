## /heal: sitting down and standing up (2026-11-05)

From FFXiMain.dll (Steven's unpacked copy, all_decomp.c) and the Hume M skeleton file 7072 (ROM/27/82).

- FUN_1008b460 (the entity's transition machine, run from FUN_1008e9a0 before and after the applied status +0x170
  takes the server's +0x16c): state 0 with an applied status of 0x21 or 7 goes to state 7. State 7 waits for
  FUN_100d12d0 (the model ready) and for no `waot` on the actor (vt+0x2a0), then starts scheduler `res0` (vt+0x298)
  and goes to state 8. State 8 starts `res2` once the applied status is no longer 0x21 / 7 and goes back to state 0.
- FUN_1008b3a0 (read by FUN_100964f0, the local walk's gate in FUN_100a6240) refuses movement in states 7 and 8, and
  FUN_100a5ec0 skips the walk while op 0x2e holds the actor (vt+0x310).
- The schedulers (7072): `res0` 145 ticks: op 0x1f 145, op 0x07 145, op 0x2e 145, `rx0` for 52 ticks, then `rx1` 93.
  `res2` 56 ticks: op 0x1f 56, op 0x2e 56, op 0x07 56, `rx2` 56. `res1` is `rx1` alone. So standing up from a rest
  takes 56 ticks (0.93 s) in which the character cannot walk.
- Ours (client/rest.rs): we never played `res0` / `res2`, only the status motion `rx1`; the server's status 0 let the
  walk start at once, and the walk's motion replaced the blend back to standing. Now each actor runs states 7 / 8 per
  frame (`res0` once its model is in, `res2` on leaving healing) through actmotion::start plus the scheduler's holds in
  fx, as synth.rs does for `res0` / `res2`. Our own walk (keys, pad, autorun, follow, click-to-move) stays held for
  `res2`'s 56 ticks from the frame it starts, under the same `move_lock` rule as op 0x2e (the scheduler's own op 0x2e
  starts a frame later).
