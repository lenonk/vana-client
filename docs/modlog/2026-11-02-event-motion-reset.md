## Event ops 0x5f sub 2 / 0xc1: reset the named actor's event motions (2026-11-02)

- Op 0x5f sub 2 and op 0xc1 run the same FUN_100b5510 (0xc1 through FUN_100b5500, from the dispatcher table at
  0x100bc960; the old FUN_100b3870 names were off). It takes the actor operand after the sub byte (0x5f: pc+2;
  0xc1: pc+1). When the event binds that actor (+0x120 bit 7) and it has a model (bit 9), it waits for the model
  (FUN_100d12d0), stops the actor's event scheduler (FUN_10087d40: vtable +0x29c on +0x1ec / +0x1f4) and drops its
  motion files of kind 2 (FUN_100d44b0(2)). 0xc1 ends the frame after it; 0x5f does not.
- Ours applied 0x5f sub 2 to the thread's own actor instead of the named one, so the 204 uses (every one names an NPC
  other than the block's own) reset the wrong actor and left the named NPC's event motions running. It now takes the operand,
  checks that the actor is bound and has a model, and handles 0xc1 the same way.
- Traced, unchanged: op 0xac sub 4 (FUN_100bb180) ORs val@6 & 3 into actor@2's +0x13c, which the event teardown
  reads (FUN_100ae3f0 at 0x100ae422); its only uses (zone 280, four) pass 0, so it does nothing. Op 0x5c 0xb0 /
  0xb1 (FUN_100b6e30): 0xb0 stores whether the streamed BGM is playing (FUN_10246710(0, -1): 1 / 0, -1 without
  the stream player); 0xb1 restarts the current BGM (FUN_100378b0(DAT_10463940)) when it is not. Used by ambient
  loops in zones 210, 232, 236 and 239; ours stores nothing for 0xb0 and ignores 0xb1. That needs a "music playing"
  query from the audio thread.
