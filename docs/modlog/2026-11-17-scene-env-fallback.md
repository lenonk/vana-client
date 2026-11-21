## Weather: a scene whose weather data gives no env is lit by DAT_1065dc10 (2026-11-17)

<!-- dll-build: 30261102_0 -->

The point "Event scenes: data/weat, the first-folder fallback, the default env" (2026-11-15) put the default env in
only when a scene had no weather table at all. `ZoneWeather::parse` returns an empty table (not an error) for a scene
with no `weat` folder, so the live client kept that table and drew the scene unlit; only zone-shot, which checks
`at()`, took the default. All addresses are build 30261102_0.

**Retail.**
- Each frame FUN_10188a70 hands an area its weather object's env (+0x10 -> +0x20) through FUN_10189690, which takes
  DAT_1065dc10 for a null pointer. The weather object (FUN_1018bd70) starts with +0x20 at 0 and FUN_1018b910 sets it
  only from the keys it finds (FUN_1018b400), so a scene with no key for the hour is lit by DAT_1065dc10, whatever
  the reason. A weather with no folder of its own is not such a case: FUN_1018bd70 falls back to `suny`, then to the
  whole `weat` folder (`ZoneWeather::at`). Every zone and event-scene DAT has keys in each weather folder it has
  (checked over all 349 scene files), so "no key" means "no `weat` folder".
- DAT_1065dc10 is the default FUN_1018c740 fills (`Env::retail_default`), until op 0x7e: FUN_1018c5e0 copies the
  scoped 0x2f block over DAT_1065dc10 itself (FUN_1018bd30, 0xac bytes) and points every area at it. So with no env
  from the weather, the static env shows when one was set, else the default; ours put the default ahead of the static
  env for a scene with no table, and nothing for a scene with an empty one.

**Ours.** `Env::scene(weather, static_env)`: the weather's env, else the static env, else `Env::retail_default`;
`world_lighting` takes it for any loaded scene, with or without a table.

**Open question.** Op 0x7c (FUN_10188ac0 in this build) makes a new weather object and leaves DAT_1065dc10 alone, so a
static env set by op 0x7e still shows under a later weather that has no key; event.rs clears `static_env` on op 0x7c.
Not changed here.

Test `scene_without_weather_is_lit`.
