## Event op 0x69 mask 0x10: the chat call sounds' level (2026-11-03)

- Retail (docs/re/notes/2026-11-03-modlog-gaps-batch5-build.md §4): op 0x69 with mask 0x10 sets DAT_10351710 to the
  level (1 if u8@1 is 0, else 0). FUN_100380f0, the chat `<call>` sound, plays at DAT_10351710 x 127; at 0 it starts
  silent and its task (and the pad rumble) ends the next tick. The sound init sets 1; nothing else resets it (not the
  event end, not a zone change). Op 0x6a ignores mask 0x10.
- Ours ignored mask 0x10 and played call sounds as interface sounds (at the mask 2 level). Now `Audio::event_volume`
  keeps the level for a 0x69 with mask 0x10 and the party call tags go through `Audio::play_call` at it (effects
  setting x call level, not the interface level). At 0 ours plays nothing. The received-tell sound (ours) stays an
  interface sound. event_ops.rs's 0x69 text names the channels as this build has them.
- Not done: the call task's one-at-a-time slot (120 ticks) and its rumble pulses.
