## Gamepad: B / Shift held is a strafe lock, not a turn to the camera (2026-11-04)

From FFXiMain.dll (Steven's unpacked copy, all_decomp.c / full_text.asm).

- Key function 0x9c (LShift, or the pad's cancel slot 3 = B) is read only by FUN_100a6240 as a held test
  (0x100a66fb, 0x100a70d9), gated by FUN_1025e070 (config 0x42 == 2: the control type, set by the controls menu
  FUN_1019b200 items 5 / 6 / 0xb = 0 / 1 / 2; default 2, FUN_10195460(0x42, 2, ...)) and the player's vt+0x330 (free).
  In types 0 / 1 it sets the flag only while the stick's X is past the dead zone, and saves no offset.
- Held: FUN_1025e090 saves the camera's offset (FUN_10015250 = the camera, DAT_104568fc +0x50; +0x44 eye minus +0x50
  look) at 0x1067a288 (a fresh press saves it as is, later frames keep its direction at the current length), and
  vt+0x344(1) (0x100a46a0) sets actor +0xfb. FUN_100a5e10 (every frame) then puts the eye at look + offset, so the
  camera keeps its world angle; FUN_1001ee60 turns off the stick / 0xa9 / 0xaa camera turns and the collision check
  while +0xfb is set; FUN_100a6240 returns before the turn to the move direction (heading param_1[0x3a] kept) and picks
  a direction-relative motion (FUN_100a80d0, param_1[0x166]). So holding B strafes / backpedals; it neither turns the
  player to the camera nor re-centres the camera. A tap does nothing visible. FUN_101252f0(1) only marks the mouse
  pointer active.
- No window check on this path (0x9c's context mask is 0xffff), so retail does the same with a window up.
- Ours: the heading no longer turns to the camera while 0x9c is held (mod.rs update_movement). The strafe / backpedal
  motions (FUN_100a80d0) follow in the next change.
