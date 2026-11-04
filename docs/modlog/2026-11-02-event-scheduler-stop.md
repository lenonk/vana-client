## Event op 0x50: stop an actor's scheduler (2026-11-02)

- Op 0x50 (FUN_100b4d10, current build 30261006_0), 13 bytes: actor@1 and actor@5 must both be in the zone with a
  model (+0x120 bit 9), then actor@1's model vtable +0x29c(4cc@9, actor@5's model). For CXiSkeletonActor (vtable
  0x10330f40) that is FUN_10084cc0: the type-7 resource of that name, the actor's own files first (FUN_100ce480), then
  FUN_10056ef0 deletes every CMoSchedularTask ('sM') of it on that pair (actor1, actor2). The task's destructor
  (FUN_100574a0) only drops its actor references (vtable +0x16c = FUN_100833a0, a count): the motion already playing
  is not reset.
- Used 98 times in 21 zones of 0..299, after a 0x2c / 0x5b start: doors' 'open' (wait 90, stop) in the cities, an
  NPC's 'tlk0' stopped once its line is closed (Port Bastok npc005).
- Ours: an event start still loading loses its name, a running Play stops where it is (no later motion, not busy for
  0x53; its current motion runs on), and the effects side stops (Effects::stop_actor_scheduler). Tests:
  `scheduler_stop_keeps_current`. Not checked live.
