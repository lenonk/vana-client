## /prcmd add while /anon or /away (2026-11-03)

- Checked in Steven's unpacked FFXiMain.dll (sha256 8097dbf8..., 2026-11-03): the own actor's +0x124 bit 22 is
  Away and bit 23 Anonymous (LSB nameflags AWAY 0x4000 / ANON 0x1000). Writers: s2c 0x037 u32 @0x28 bits 7 / 5
  (FUN_1009d0f0, the normal path), 0x00A u32 @0x20 bits 14 / 12 at zone-in (via G+0x40db0 bits 4 / 3), 0x00D mask 4
  @0x20 bits 14 / 12. MODLOG's "0xc6" is message 7 0xc6, not an s2c packet (no 0x0C6 handler is registered).
- /prcmd add (asm 0x1007da8b..0x1007dab7): with either bit set, 7 0xc6 "You were unable to make a request to join."
  and no c2s 0x11C. The name-lookup path (LAB_1007dc00) does not test them. Ours: textcmd::prcmd from
  `World.self_flags` (0x037 @0x28).
- Other readers, not done here: the name colour FUN_1008db40 (bit 23 -> the anon colour DAT_104e1ba4) and the
  name glyph FUN_10097500 (bit 22 -> PC glyph 0x90, the away icon).
