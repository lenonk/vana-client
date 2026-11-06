## Strafing while locked on; control types 0 / 1 (2026-11-05)

From FFXiMain.dll (Steven's unpacked copy).

- Control types 0 / 1 (config 0x42; FUN_101b7ee0: 1 and 2 turn on the compact keyboard table, so 0 full keyboard, 1
  compact, 2 compact + the strafe lock) move the same in third person: FUN_100a6240 skips only the FUN_1025e070 block,
  nothing else sets +0xfb, and free movement (+0xf9 = 1) is camera-relative (FUN_100a79a0) with the heading turned to
  the move. So B does nothing for movement there. Ours already matches.
- Locked on (+0xf9 = 0, set by the lock callbacks 0x100c5440 / 0x100c5410), FUN_100a7b80 moves relative to the target
  (forward / back the distance, X orbits) facing it, in every control type: +0x598 from FUN_100a80d0 against the way
  to the target (0x100a7ef9), FUN_100a8000's speeds (back a third, sideways 0.0625 / 0.125 riding), and since vt+0x330
  is false FUN_100c8bb0 plays mvr / mvb / mvl. A sudden 2 <-> 4 flip forces 1 for +0x5b1 frames (0x100a7f23) *(not
  done in ours)*.
- Ours: update_movement's lock_dir and World::my_strafe now also run locked on. The quarter is read from the stick in
  the locked step's own frame (forward toward the target, target::locked_step), not against each frame's way to the
  target: a monster's position jumping in melee range swung that between sideways and forward every frame, cutting
  mvr / mvl to the run and back (Steven's log, 2026-11-06).
- FUN_10084400 is the actor's model kind ([+0x70]+0xee): 2 / 7 are fixed models without the mv? motions, so
  FUN_100c8bb0 (0x100c8d5e) plays the move motion (+0x7cc) for 2..4 instead. Ours: an actor drawn from one model
  id (Source::Npc(NpcLook::Model), mod.rs fixed_model) takes its walk.
- Other actors: only local-player code writes +0x598 or +0xf9 / +0xfb (the base vtable 0x1032e3c8 returns 1 / 0), so
  retail never plays mv? on other players or NPCs. (FUN_1004a980 writes +0xf9 / +0xfb on some script object at
  0x1004bce3 / 0x1004bd7d, unconfirmed as an actor.)
- First person (DAT_10487f80, key 0x10): tank controls, stick X turns (heading += 2 x X x speed x 0.0174528), Y moves
  along the heading; 0x9c held with |X| > 0.01 zeroes the turn and X moves sideways (vt+0x344(1)), camera FUN_10021110;
  +0x598 not updated. Done in ours: see "First-person tank controls".
