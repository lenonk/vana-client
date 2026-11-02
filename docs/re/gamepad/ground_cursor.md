# Ground-position cursor, the field command table and L3 on the map

From the unpacked FFXiMain.dll (30261006_0). [C] confirmed in code, [I] inferred.

## Position-target mode

- Field object `*DAT_1057876c`. `+0x58` = position-target mode, set only by FUN_1022b850(spell) as
  `(rec[+0x44] >> 5) & 1`, rec = DAT_10667a74 + spell x 100 (the magic table) [C]. Callers: the magic list
  (FUN_1022c7a0) and /magic's prompt path (0x1007b7a8) [C]. Bit 5 of +0x44 marks the GEO-main spells (the Geo- spells)
  [I from the table's other use, the Entrust check in FUN_101022e0].
- `+0xc4` = the 3D target prompt's callback (FUN_10159440). `+0x59` = party-window targeting (FUN_10202d80), which
  zeroes +0x58 [C].
- Cursor: `+0x10 / +0x14 / +0x18` position, `+0x22` active [C]. A new pick (FUN_10159660) clears +0x22; cancel
  (FUN_10159740) ends the prompt [C].

## Moving it

- Key functions 0xb0 / 0xb1 / 0xb3 / 0xb2 (pad: LT or RT held + d-pad up / down / left / right; keyboard: the arrows
  with flag 0x10 = (Ctrl or Alt) and DAT_1036cf5c, which defaults to 1 and is cleared by the key-assignment screen [I])
  -> field commands 0x18..0x1b [C].
- FUN_10159970: direction up (0, 0, +1), down (0, 0, -1), right (+1, 0, 0), left (-1, 0, 0) [C].
- FUN_10159a20: the first move copies the current target's position; step = dir x config 0xc4 x frame ticks
  (`__ftol(FUN_10014cf0())`, at least 1) x 0.01, rotated by FUN_10159b10 [C].
- FUN_10159b10: t = atan2(look.x - eye.x, look.z - eye.z); x' = x cos t + z sin t, z' = -x sin t + z cos t. Up is the
  camera's level forward, right is (f.z, 0, -f.x), the screen's right [C].
- Height: FUN_10177770 -> FUN_10181210, a segment from (x, y - zone+0x170, z) to (x, y + 17, z) (y points down;
  zone+0x170 = 2.0, 0.5 in zone 0x95, set at zone load by FUN_10178df0). FUN_101693b0 keeps the hit nearest the start
  (the highest surface). No hit: y unchanged [C].
- No range clamp in the mover [C]; FUN_10150610's distance test only tints the arrow [I].
- Mode 0 = the REPEAT test: the press, then the repeats (past 16 ticks strictly, then the timer goes back to 14:
  tick 17 and every 3rd tick) [C].
- Analog: with the keyboard's Ctrl / Alt held (0x76 / 0x77), FUN_10159ba0 moves the cursor every frame by the movement
  stick (axes 4, -5), else the camera stick (6 / 7), same step formula; player movement and the camera ignore those
  axes meanwhile [C].
- Config 0xc4: FUN_10195460(0xc4, 10, 4, 0, 1, 100, 10, 0): range 1..100, default 10. No Config window writes it [C].

## What it does

- Confirm (A, command 1) runs FUN_1022d140: `/magic "<name>" <target id>`. The /magic handler (0x1007b710) takes
  FUN_10159cc0 = cursor - target position (zero with no cursor) and sends c2s 0x01A (0x1c bytes): +0x04 target id,
  +0x08 target index, +0x0a category 3, +0x0c spell id, +0x10 / +0x14 / +0x18 f32 x / y / z offset [C].
- Drawing: the animated target arrow (FUN_1014ffe0) at the cursor's screen projection, 0.5 yalm and 10 px up
  (FUN_10159d40) [C]. FUN_100ca730 places an actor's companion model (actor[0x1e5]) at the cursor with the actor's
  pose [C]; what creates that model was not found [I: a preview].

## The field command table 0x10371ee0

- Dispatched every field frame by FUN_101585f0 when (+0xc4 or +0xc8 set, or (+0x74 == 0 and +0x6a < 1)) and not
  (+0x74 & 2); +0x6a counts the open windows holding the field [C]. The 0x3b / 0x3f first field is ignored
  (FUN_10123a70 never reads it); an id acts when its mask (0x1036cf60) & the input context DAT_1036e3a0 is set, and
  the table's ids have mask 1, so the field context 0xffff only [C].
- FUN_101587b0 returns the first entry whose test passes, one command a frame. Order: 0xb0, 0xb1, 0xb3, 0xb2, 0x91,
  0x27, 0x28, 0x2a, 0x29, 0x4c, 0x46..0x4b, 0x52, 0x53, 0x54, 0x84, 0x97, 0x4d, 0xab, 0xac, 0x4e, 0x92, 0x8b, 0x8c,
  0xb8 [C]. So a trigger + d-pad hits 0xb0..0xb3 first and never steps the target; outside a ground prompt their
  command does nothing (+0x58 == 0) [C].
- Locked on with no prompt, FUN_101587f0 swallows every command but 1 and 5 [C].
- Up / down (command 2, FUN_10158b60) read field +0x60: & 4 party allowed, & 8 the alliance too (else the party
  only), & 0x200 self. Left / right (commands 3 / 4) are FUN_10083b40 / FUN_10083d00(current, +0x60, +0x64) [C].
  +0x60 / +0x64 come from FUN_10159170 and table 0x103720c0 (1 -> 0x200, 2 -> 0x100, 4 -> 4, 8 -> 8, 0x10 -> 1,
  0x20 -> 0x10, 0x40 -> 2, 0x80 -> 2 with +0x64 = 0x1800); a party target also gets 8 when FUN_101cc350 says we are
  in an alliance [C]. With no prompt +0x60 = 0xffffffff and +0x64 = 0 (FUN_101578f0 / FUN_10159040) [C].

## L3 on the map (key function 0x7e)

- FUN_101ef0a0 calls FUN_101ef990((short)view+0x20, (short)view+0x22), which adds the frame inset (+0x7c / +0x7e =
  0x1e) and eases that point under the cursor point, clamped to the map [C].
- On the zone map (mapframe, DAT_10663888) FUN_101f8ea0(1) sets view+0x20 / +0x22 to the player's map pixel
  (FUN_101f8fb0: world x / z -> FUN_101f7570 -> +0x67a / +0x67c) from the open / set-up FUN_101f9a50 [C]. So L3 goes
  back to the player as of the open, not a live position. FUN_101e8c00 is the region-type map's (DAT_10663648) [C].
- Context masks: 0x7e = 4 (acts on the map's context 0x26), 0x30 = 8, 0x44 = 8 (not on the map) [C].
