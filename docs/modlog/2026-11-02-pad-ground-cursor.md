## Gamepad: the ground-position cursor; L3 on the map (2026-11-02)

From FFXiMain.dll (docs/re/gamepad/ground_cursor.md).

- Position-target mode is field (*DAT_1057876c) +0x58, set only by FUN_1022b850(spell) as the magic record's +0x44
  bit 5 (dat::actions::Spell::flags 0x20: the Geo- spells). The magic list (FUN_1022c7a0) and /magic's prompt
  (0x1007b7a8) call it; the party-list cursor (FUN_10202d80, field +0x59) zeroes it. So only the 3D target prompt
  (+0xc4) of such a spell has the cursor. Ours: target::Sub::ground, target::ground_mode.
- The cursor (field +0x10 / +0x14 / +0x18, live while +0x22 is set; World::ground_cursor) starts at the pick's position
  on its first move. Key functions 0xb0 / 0xb1 / 0xb2 / 0xb3 (LT or RT held + d-pad up / down / right / left; Ctrl or
  Alt + the arrows with DAT_1036cf5c, 1 unless the key-assignment screen clears it) move it, mode 0: on the press and
  the repeats. A step (FUN_10159a20) is dir (up +z, right +x; FUN_10159970) x config 0xc4 (1..100, default 10, no
  Config row) x the frame's whole ticks (at least 1) x 0.01, turned by atan2(look.x - eye.x, look.z - eye.z)
  (FUN_10159b10: x' = x cos + z sin, z' = -x sin + z cos; up is the camera's level forward, right the screen's right).
  Its height (FUN_10177770 -> FUN_10181210) is the highest collision face on a segment from 2 above (zone +0x170;
  0.5 in zone 0x95) to 17 below the cursor; no hit keeps it. The mover has no range clamp.
- With the keyboard's Ctrl / Alt held (0x76 / 0x77), FUN_10159ba0 moves it every frame by the movement stick (axes
  4, -5), else the camera stick, with the same step; we and the camera take neither meanwhile.
- A new pick resets it (FUN_10159660); cancel ends it (FUN_10159740). Confirm runs `/magic "<name>" <id>`
  (FUN_1022d140), and the /magic handler (0x1007b710) sends c2s 0x01A category 3 with the cursor less the target's
  position (FUN_10159cc0; 0 with no cursor) as f32 x / y / z at +0x10 / +0x14 / +0x18.
- The pick's arrow (FUN_1014ffe0) stands over the cursor, 0.5 up and 10 px up (FUN_10159d40), tinted by range as
  before. FUN_100ca730 also places an actor's companion model (actor +0x1e5) at the cursor with the actor's pose;
  what creates that model was not found, and ours draws none.
- The field command table 0x10371ee0 (FUN_101585f0 -> FUN_101587b0) runs every field frame in a target prompt or with
  no window holding the field, and takes only the first entry whose test passes: 0xb0, 0xb1, 0xb3, 0xb2, 0x91, 0x27,
  0x28, 0x2a, 0x29, ... So with a trigger held the d-pad never steps the target. Fixes to ours: the pad's d-pad steps
  (0x27..0x2a) now run in a target prompt too, and on the press and its repeats (mode 0), not the press only.
  Up / down (FUN_10158b60) do nothing without field +0x60 & 4 or a pick. FUN_10203920 walks all 18 slots to the
  next occupied one; without 8 (party_only) a pick or landing outside our six gives nothing. Without 0x200, landing
  on us steps once more (us again: nothing); with 0x200, nothing becomes us. Ours: target::party_pad_step.
- In a target prompt the triggers and Ctrl / Alt open no macro palette: the macro gate FUN_101a33e0 needs field
  +0xc4 and +0x59 clear, and every palette path in FUN_101a2e80 (LT 0x89 / RT 0x8a, Ctrl / Alt 0x76 / 0x77,
  FUN_101a3500) tests it. Ours: the pad's and the keyboard's palette gates leave out World::prompt, so trigger +
  d-pad reaches 0xb0..0xb3. A pick other than the cursor's drops it (target::drop_stale_ground_cursor), so going
  back to an earlier pick starts afresh.
- The windows' pad repeat is the press, tick 17 and every 3rd tick after (past mgr +0x94 = 16, strictly, then back to
  14), not 16 and every 2.
- L3 on the map (key function 0x7e, mask 4: the map's context 0x26; 0x30 / 0x44 have mask 8, so it neither rests nor
  locks on there): FUN_101ef0a0 eases view +0x20 / +0x22 under the cursor point (FUN_101ef990). FUN_101f8ea0(1) sets
  that to the player's map pixel when the map opens or is set up (FUN_101f9a50), so L3 goes back to where we stood
  then, not a live position. That is map::MapWin::home, so it already worked through the pad word.
