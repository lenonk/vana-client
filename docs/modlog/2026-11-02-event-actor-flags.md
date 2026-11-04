## Event actor flag ops 0x84 / 0x86 / 0x95 / 0x96 / 0xa4 (2026-11-02)

- Traced in the current build (30261006_0) through the dispatcher's jump table (0x100bc960). The handler names `event_ops.rs`
  gave these ops pointed inside other functions and are corrected. The real handlers: 0x84 FUN_100b9100, 0x86 FUN_100b8f30,
  0x95 FUN_100b9c20, 0x96 FUN_100b9c70, 0xa4 FUN_100ba8d0, 0xab FUN_100bac90. Ours had no handler for any of them.
  Sites in zones 0..299: 0x84 131 (40 zones), 0x86 208 (30), 0x95 267 (54), 0x96 166 (49), 0xa4 446 (81).
- 0x84: the event actor's +0x12c bit 0. The entity pass (FUN_10095900 -> FUN_10095da0) then drops its model
  (FUN_10092900), makes it again (FUN_1008f740) and clears the bit. Ours makes the model again at once (the same
  path as op 0xab 0x11's rebuild).
- 0x95 val@1 / 0x96: the event actor's +0x12c bit 19 set with bits 20..21 = val & 3 (FUN_10087d80) / bit 19 cleared
  (FUN_10087db0), then its model goes (FUN_10092900) and comes back. While bit 19 is set, FUN_10087dc0 returns bits
  20..21 instead of the 0x00E MonStat (+0x124 bits 13..14). Ours: `Entity::ev_mon_stat`, read by the MonStat 2
  idles and motion names. MonStat 1 (another model class at init, FUN_100c5880) and 3 (FUN_1008f250) are not
  modelled for 0x00E either.
- 0xa4 u8@1: the event actor's +0x12c bit 26. FUN_100c63d0 then sets the model's near-camera fade (+0x664, from
  FUN_100cd040) to 1 each frame, so the eye close to it never fades it. Unbinding clears it (FUN_100ae3f0).
- 0x86 u8@1, actor@2: actor@2's +0x12c bit 3 (only with a model, +0x120 bit 9). While the actor is bound and an
  event runs, FUN_1008cd90 then skips the turn steps ('@tl0' / '@tr0', FUN_1008af60). Unbinding clears it. Ours
  keeps the set per event, so an actor flagged while unbound does not carry it into a later event *(retail would)*.
- 0xab 9 / 0xa (FUN_10087120): DAT_10487ef5 = 1 / 0 (cleared on zone load, FUN_10093720). While it is set,
  FUN_10002d80 returns 1.0 instead of the weather effects density (DAT_10456a68), which weather generators (+0xde
  bits 7 / 1) run by. It shows weather effects even with the Weather setting (config 0x3b) off *(inf: the setter
  call at 0x101aa703 was not tied to 0x3b)*. Ours always runs weather at 1.0 (fx/mod.rs), so nothing changes and
  the ops stay unhandled until that setting is wired.
- Test: `actor_flag_ops` (the operands reach the host). The event sweep runs on the scripted host, which these
  handlers do not touch. Not checked live.
