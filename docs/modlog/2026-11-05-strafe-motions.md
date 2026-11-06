## Strafe and backpedal motions (2026-11-05)

From FFXiMain.dll (Steven's unpacked copy; 0x100c6b70 / 0x100c4390 are inside FUN_100c63d0 / FUN_100c4380).

- FUN_100a80d0(move, a point ahead on the heading), called by FUN_100a6240 only while vt+0x340 (actor +0xfb, the
  strafe lock) is set, stores the step's quarter at +0x598 (0x100a6ee9): A / B = the heading turned +/- pi/4, the signs
  of dot(A, move) and dot(B, move) give 1 forward, 2 right, 3 back, 4 left (90-degree sectors on the axes, no dead
  zone; 0 under 0.001). It never writes +0x594 (the stick magnitude, FUN_100a78d0). Idle sets +0x598 = 0; normal
  movement never computes it.
- FUN_100c8bb0 (from FUN_100c83c0) picks the walk / run as usual, then while +0xfb is set: on foot 2 'mvr ', 3 'mvb ',
  4 'mvl '; riding (status 5 or 0x55) 3 'mvb ', 2 / 4 'wlk '; FUN_10084400() == 2 takes the walk for 2..4 instead. The
  rate is the walk / run's. No fallback: a model without the motion keeps its last one (only 'id1 ' and 'df1 ' retry).
- FUN_100c63d0 (~0x100c69e9): our model with +0xfb set, not riding, eases toward the heading, not the step.
- FUN_100a8000 (after FUN_100a80d0): back = speed x 1/3 (vt+0x1b0) / 60 (x2 riding); sideways a fixed 0.0625 a tick
  (0.125 riding), whatever the speed or the stick. Ours: the same under the strafe lock (update_movement's step);
  the right drag (ours, not retail) keeps the normal speed.
- Ours: World::my_strafe (motion::strafe_dir) while the strafe lock is held, and also while the right drag holds the
  facing (ours, not retail), and motion::strafe over our walk / run. The FUN_10084400 == 2 case is not done.
