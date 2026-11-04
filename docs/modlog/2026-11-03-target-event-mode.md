## Target window: event op 0xb4's named gauge (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch12.md item 79): with the target window's +0x65
  set, FUN_101501a0 draws only a gauge (FUN_101cb090 at (L + 0x1c, T + 0x1d), +0x66 percent) and the name +0x14,
  centred, unsplit, in +0x68.
  - Event op 0xb4 sub 2 (FUN_100b0a00, case 0x100b0b94) sets +0x65 = 1, the 16-byte string var@2 as the name (byte
    15 = 0), white 0x80808080, and val@4 as the percent only when it is 0..100. It opens "menu    targetwi" unless it
    is up. pc += 6.
  - Sub 3 (0x100b0e45) closes the window. Its close hook 0x10150910 clears +0x65.
- Ours: the VM hands sub 2's string and value to the host (`event_vm.rs`, test `target_event_mode_b4`).
  `World::target_event` holds them until sub 3. The HUD draws the target window from it, whatever is targeted:
  the gauge at the percent, the name centred in white, no plate glyph.
- Not changed:
  - Fishing. The note says FUN_10088bd0 puts "Fish" in the same mode (+0x65 = 2), but ours keeps the gauge
    MODLOG's fishing entry describes (FUN_10202af0, under the party window). Which one is right needs a live check.
  - The pet line ("name" / "(master)"). Its master search reads actor +0x74, +0x1fa and +0x1d0 bit 0x100, which
    aren't mapped to our fields yet.
