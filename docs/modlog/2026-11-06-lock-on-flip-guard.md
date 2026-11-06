## Lock-on flip guard (2026-11-06)

From FFXiMain.dll (Steven's unpacked copy), full_text.asm, FUN_100a7b80.

- 0x100a7edf keeps the old +0x598 (last frame's, after any forcing; the idle paths in FUN_100a6240 at 0x100a65b3 /
  0x100a6c05 / 0x100a7319 and the constructor write 0), FUN_100a80d0 gives the new quarter (0x100a7ef1), and only
  2 -> 4 or 4 -> 2 sets the signed byte +0x5b1 to 2 (0x100a7f10).
- While +0x5b1 > 0 it drops by one a call of FUN_100a7b80 and +0x598 is 1 (0x100a7f17..0x100a7f2d): forward for the
  flip frame and the next, then the real quarter. No tick factor; nothing else touches +0x5b1 but the init (0x100a8ba6).
- The facing (0x100a7f85) and FUN_100a8000 (0x100a7fd4, reads +0x598 at 0x100a801b) come after, so those two frames
  run at the forward speed and face the target. The strafe lock's block (0x100a6ee4..0x100a6ef6) has no guard.
- Ours: motion::lock_flip_guard in update_movement's lock_dir, 4 ticks for retail's two frames *(inf: 30 fps
  frames)*.
