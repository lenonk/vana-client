## GroundFlag and HackMove stop the ground snap and tilt (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch6.md item 14): the two unknown bits are LSB's
  server flags.
  - +0x120 bit 20 GroundFlag: s2c 0x00D / 0x00E u32 @0x18 bit 15, s2c 0x037 u32 @0x28 bit 2, at zone-in char
    +0x40dac bit 13, and a locally made actor sets it.
  - +0x128 bit 14 HackMove: s2c 0x00D u32 @0x20 bit 27, s2c 0x037 u32 @0x2c bit 12, at zone-in char +0x40de4 bit 0.
  - FUN_10084640 is HackMove | +0x120 bit 21 | GroundFlag. When it holds, the model gets no ground tilt
    (FUN_100cbde0), and the event snaps (ops 0x36 / 0x37 / 0xba and the walks) leave the height alone.
- Ours: net.rs parses no_snap on the entity (0x00D / 0x00E) and on our own plate (0x037). client/event.rs's ground
  hook returns None for such an actor, so every event snap site skips it, and client/mod.rs skips the ground tilt.
- Not built: the zone-in sources (char +0x40dac bit 13 / +0x40de4 bit 0) are not mapped to 0x00A fields. LSB sends
  0x00D / 0x037 right after zone-in, so the flags arrive a frame or two later.
