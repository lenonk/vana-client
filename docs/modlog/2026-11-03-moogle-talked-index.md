## Moogle motions: the NPC we talked to goes first (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch6.md item 17): S+0x40daa holds the index of the
  NPC we last talked to. The talk action FUN_1007bf30 writes it after the request goes out, unless we are dead
  (status 2 / 3). It is cleared at zone-in (the 0x00A handler), at the event end (FUN_100ae3f0) and per frame by
  FUN_10096640 once that actor is gone.
- The moogle actor FUN_100a0080 tries that index first, so with several moogles in range the one spoken to plays the
  `17 n` motion. FUN_100a0010 also requires the actor kind +0xee to be 1 (a PC-look NPC marked talkable), 2 (a
  model-id NPC) or 7 (0x00E SubKind 6) beside the model check.
- Ours: World::talked_index, written by client/event.rs's talk, cleared on zone-in, at the event end and when the
  actor leaves. moogle::moogle_actor prefers it and now applies the kind test (our sub_kind 0 / 1-with-npc_pet / 6).
- Not built: retail also sets a "watch it" flag when the target's +0x128 bit 5 is clear, and clears the index when
  the actor becomes both talkable and +0x124 bit 12. Neither changes which moogle plays in practice.
