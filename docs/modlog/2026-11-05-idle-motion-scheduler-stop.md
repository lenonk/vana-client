## Events: idle motion ops stop the event motion scheduler (2026-11-05)

- Ops 0x6b (`FUN_100b7070`) and 0x5e (`FUN_100b71e0`), reached through the jump table at 0x100bc960, call
  `FUN_10087d40` on the actor before they set the idle motion (+0x1a4, model +0x7c8, +0x7d8 = "    ").
  `FUN_10087d40` calls model vtable +0x29c with actor +0x1ec (scheduler 4cc) and +0x1f4 (second actor's model).
  It needs both fields set and +0x120 bit 9. This is the same call that op 0x50 makes, and it deletes the scheduler task.
- +0x1ec / +0x1f4 are written only by the motion op body `FUN_100b5210` (0x100b5466). Ops 0x5b / 0x66 and 0x5f
  subs 3..6 reach it. Unbinding clears both (0x100bd355).
- Client: `State.event_sched` keeps the last motion-op scheduler for each actor, and `Effect::IdleMotion` stops it
  through `LiveHost::stop_scheduler` (the op 0x50 path) before it cuts the motion.
- Seen in the Records of Eminence intro (Bastok Markets 235 event 24). The memorandoll (npc 0x0b2, model 970) gets
  `motion_file #252 'bru0'` and `set_idle_motion 'idl0'` 120 ticks later. Scheduler `bru0` (file 32356) plays `bru` once,
  then loops `br1` (`br10`, a shake) from tick 480. Before this change that loop started anyway and ran to the end.
