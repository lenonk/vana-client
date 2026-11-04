## Invisible and linkshell refusals (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch6.md item 19): +0x128 bit 6 is InvisFlag (LSB
  Flags1 bit 29). It comes from s2c 0x00D / 0x00E u32 @0x20 bit 29, s2c 0x037 u32 @0x2c bit 15, and at zone-in from
  char +0x40db2 bit 1. FUN_1007bf30 (talk, and the target actions that share it, e.g. /returntrust) refuses with
  7031 0xac "You cannot use that command while invisible." after the mount refusal (7 0x4b) and before the range test.
  FUN_100e3400 refuses /l, /l2 and the third shell with 7031 0x25 when that shell isn't equipped, and /p with
  7031 0x24 with no party; the typed line is still echoed.
- Ours already had the talk refusal, the linkshell and party refusals, and 0x037 as the source of our own flag.
  /returntrust and /returnfaith now refuse with 7031 0xac too (client/textcmd.rs).
- Not built: the zone-in source. Which 0x00A field fills char +0x40db2 bit 1 is not traced. LSB sends 0x037 right
  after zone-in, so our flag follows within a frame or two.
