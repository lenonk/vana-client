## Home point set effect: op 0x2c's target (2026-11-04)

- Setting a home point plays no effect on us. LSB sends nothing for it: the home point event itself (e.g. Port Bastok
  event 0x21fc, `call ->0210`, choice "Yes.") runs op 0x2c `actor_sched self, player, 'bind'`. The crystal (model 51,
  file 1351) has a `bind` scheduler (type 7) and its `bnd0` generator.
- FUN_100b2f80 calls the actor's model vtable +0x298(4cc@9, actor@5's model), so actor@5 is the scheduler's second
  actor. Ours dropped it and ran the scheduler with the crystal as both actors, so the effect stayed on the crystal.
  actmotion::Pending now carries op 0x2c's target (when it is another actor in the zone) and the scheduler's effect
  side runs with it as actor2 (fx `on_scheduler_quiet_on`). Ops 0x5b / 0x66 are unchanged.
- Not yet seen live in game.
