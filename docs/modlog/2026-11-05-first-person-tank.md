## First-person tank controls (2026-11-05)

From FFXiMain.dll (Steven's unpacked copy), full_text.asm: FUN_100a6240's first-person branch (DAT_10487f80 set,
0x100a70ae..0x100a778c).

- Shared with third person: FUN_100a78d0 normalises the stick (axes 4, -5) and tiers it (> 0.9 full, > 0.05 a third,
  walk caps at a third; +0x594). Speed vt+0x1a0 (x2 riding, cap 30) / 60 x tier x the tick factor
  ([DAT_104568fc+0x2c], 60 Hz ticks over 4 frames, cap 20).
- Free (vt+0x330 true), in a moving status ({0, 1, 4, 0x1c, 0x1f} or riding, 0x100a7324) and without a /follow
  target: heading +0xe8 += 2 x X x ticks x 0.017452778 (0x100a7381; X normalised and tiered, stick right turns
  right), wrapped to +-pi, and the move goes along the new heading at once. Y forward and back at the same speed,
  backing up still facing forward. The third-person turn to the move (0x100a6f49..) does not run.
- 0x9c held (FUN_10123a70(0x3f, 0x9c, 4, -1)) with raw |axis 4| > 0.01: vt+0x344(1), X becomes the sideways move
  (0x100a728f, stick right moves right) with no turn; Y still moves, the pair renormalised. Any control type: nothing
  here reads config 0x42.
- +0x598 is only cleared when idle (0x100a7319), so FUN_100c8bb0 plays walk / run for every direction (no mv?), and
  the tier, not the step, picks the motion (FUN_100c8930): turning on the spot walks or runs in place.
- Locked on (vt+0x330 false) FUN_100a7b80 wins (0x100a769f): no tank turn. Autorun (0x100a764c): X turns the autorun
  vector and the heading by the same 2 degrees a tick.
- FUN_10021110: axes 6 / 7 look around (yaw 1.8849, pitch 1.413675, 0.25 a call, inverted by 0x8d / 0x8e) and spring
  back; eye and look ease 1 / 8 a tick. Never writes the heading. Ours already matched.
- Ours: update_movement's `tank` branch. A mouse drag's first-person look (ours) stands for the heading, and the stick's
  turn carries it along (camera::fp_turn).
