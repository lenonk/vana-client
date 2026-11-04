## Misc. 2: Animation frame rate (2026-11-03)

- Config 0x40 (Misc. 2 > animation frame rate, slider 0..3, default 3) was saved but never read. Retail
  (FFXiMain.dll, checked 2026-11-03):
  - LAB_101aa650 case 0x40 (FUN_10002e40): DAT_10456c00 = 3 - value.
  - FUN_100cbde0 (0x100cc9b4), per actor, with the frame count FUN_10014d50 (DAT_104568fc +0x34) and the actor's
    slot +0x9fc (0..3, the least used one when FUN_100c62e0 makes it): rate 1 skips the frame where count % 4 is
    the slot, rate 2 the frames off the slot's parity, rate 3 every frame but the slot's; a frame is never skipped
    twice running (+0xa00), nor while +0xa04 forces an update (set when the actor is made and by FUN_100a4b40). Not
    for an actor whose vt +0x358 holds, one on a mount (status 0x55, FUN_100956a0), or with entity +0x120 bit 7 / 8.
  - A skipped frame clears DAT_10457460, so FUN_100224d0 doesn't skin (FUN_10023290): the old pose stays.
- Ours: anim_skip; an actor other than ours keeps last frame's whole pose (AnimHeld: motion frames, reaction
  overlay frame and weight, head look) on a skipped frame, as long as it plays the same animations, and its cloth
  step waits for the next posed frame *(inf: retail may drop that tick)*. The GPU still re-skins the held pose
  each frame (the look matches; retail's saving of the skinning work is not mirrored). Slots are dealt in turn;
  vt +0x358, +0x120 bits 7 / 8, the +0x34c / +0x768 check and FUN_100a4b40's callers are not mirrored *(inf)*.
