## Costumes: drawn as the NPC model, ours and other players' (2026-11-03)

- Retail (docs/re/notes/2026-11-03-modlog-gaps-batch4.md §4, 2026-11-03-modlog-gaps-batch5-build.md §6): FUN_10095e90
  runs for every actor. Another character's costume comes from s2c 0x00D u16 @0x30 (ours from 0x037 @0x44). When it
  goes on the model is freed and reloaded as FUN_100d2f60 case 2: FUN_100c51c0(0, +0x166), the NPC model table, the
  whole body. It comes off with id 0, while bound to an event, or in one of the listed statuses; then the look is
  rebuilt.
- Ours had the state for us only (the refusal, PR #136). Now `costume::step_all` steps ours and every other character's
  (`World::costumes`) each frame. A costume going on keeps the actor's look aside (`World::costume_base`) and loads
  `Source::Npc(Model(id))`; looks that arrive meanwhile are kept aside too; coming off loads the look kept. Costumes
  reset with the actor table at zone-in.
- *(inf, as retail)* In one of the statuses that take a costume off it goes back on the next frame, so it flips each
  frame while the status lasts; models are cached, so this only swaps them.
- Not done: +0x130 bit 21 (the Mog House furniture-item actor's type-2 table).
