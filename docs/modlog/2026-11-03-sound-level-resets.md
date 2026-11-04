## Sound levels: the resets at zone-in, the event end and a slider move (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch6.md item 18): one reset a zone, guarded by
  DAT_104804c3, which the zone / world init clears. Whichever comes first sets it:
  - the zone-in, once the player's model is loaded (FUN_1008abe0): the ambient loop to full at once, the world
    effects and the interface effects over 30 ticks;
  - an event end (FUN_100ae3f0), only while the flag is still clear, i.e. a zone-in cutscene: the ambient and the
    world effects over 30 ticks. The interface effects, the stream and the `<call>` mute are left alone.
  - So after the zone's reset, a normal event's end restores nothing an op 0x69 / 0x6a lowered. It stays until
    another op, a slider change or the next zone; scripts put their own levels back.
  - A volume slider's apply (config 9 / 10, FUN_101aa650 -> FUN_10035e80 / FUN_10035ec0) puts every channel's event
    level back to 1.0 at once, so moving a slider during a cutscene undoes the script's mute.
- Ours: World::sound_reset is the flag. The zone-in path clears it, the settle step runs the zone reset, and
  event::finish runs the event-end reset only while it is clear. Our zone-in cutscene waits for the settle step
  (event::zone_ready), so the zone reset waits while an event is running or waiting: that event's end is the one
  that restores, as retail's zone-in cutscene does. An event that goes without finishing (no script for it) leaves
  the flag clear, and the zone reset then runs on the next frame. audio::reset_event_levels is called from
  apply_settings only when config 9 / 10 themselves changed (App::applied_volumes, the raw slider values, recorded at
  every apply and when the device opens); other settings and our mutes leave an event's levels alone.
- Inferred: that retail's zone-in cutscene ends before the player's model comes up (the note's reading of
  "whichever comes first").
