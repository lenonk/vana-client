## Action menu: which commands show on a target (2026-11-06)

From FFXiMain.dll (Steven's unpacked copy, full_text.asm / all_decomp.c).

- FUN_1012b4b0 picks the layout (table 0x1036f390, 13 bytes each): engaged 6; else a target FUN_101cc220 calls
  hostile takes 2 (layouts 4 / 5); an NPC (+0x1d0 bit 1) that FUN_1008e150 does not call an ally takes layout 7 when
  +0x1d0 bit 8 is set (FUN_1008e290: +0x12c bit 13, 0x00E @0x27 bit 3, a player owns it), else 0; anything else
  (players, us, allies) 1. Riding (status 5 / 0x55) doubles plus one and passes flag 1; layout 7 has no riding form.
- FUN_1012af30 (jump index 0x1012b2a8, table 0x1012b26c, command - 3) per command: 3 Disengage (Call for help while
  DAT_10578174); 4 Chat, none on an NPC (UniqueNo high byte 1), Tell on any other target; 6 Talk / 0x11 Disband /
  0x14 NPC Trade only on FUN_101cc550 (our +0x2a0: the adventuring fellow, 0x037 @0x48) while its +0x170 is not 1;
  0xa (player layouts' Check) none on us while riding, Fish (0xd) on us when FUN_10177730 and zone != 0xd3, else
  hidden by FUN_1008e150; 0xb Check needs a target, not +0x1d0 bit 8, not FUN_1008e150; 0xc Treasure hidden by
  FUN_101cc350(1, 0), else FUN_1013eff0; 0xd Fish hidden by FUN_101cc350(0, 0), else FUN_10177730 and zone != 0xd3;
  0xe Dig (mount, [S] zones FUN_100fd3a0, +0x123 & 7 tests); 0x10 Invite hidden by FUN_10201f00, else a target other
  than us with +0x124 bit 20 or 21 (0x00D flags1 bit 11 seeking party, flags2 bit 31 auto-party); 0x13 Trade hidden
  by FUN_101cc350(0, 0), else a non-NPC target; 0x16 / 0x17 Quarry / Scout need FUN_101cc350(1, 1); 0x18 Sprint the
  same, Scout with +0x27c; 0x1a Release: FUN_1012aed0, the target's +0x12c bit 22 (trust) and its +0x2a4 master (s2c
  0x067 kind 3 @0x0C) our own entity; 0x1b Request a target with +0x140 bit 3 (0x00D flags3 bit 1, recruiting).
  Every other command shows.
- FUN_101cc350(a, b): our +0x1d0 bit 10 (team 2..99), not 8 / 9 when a, not 0x28..0x2b when b.
- FUN_1012b880 (command - 1, table 0x1012bc2c): 0xd "/fish"; 0x10 FUN_100e8fd0 (c2s 0x06E, kind 0); 0x11 group 0xb
  0xb8 yes / no, Yes (LAB_1012bcc0) FUN_1007bf30(target, 1) and closes playermo; 0x1a FUN_1007bf30(target, 1) with
  the menu left up; 0x1b "/partyrequestcmd add <name>" (+0x214 when +0x210, else +0x7c). Talk (6) is
  FUN_1007bf30(target, 0) and closes playermo.
- FUN_1007bf30's second argument reaches FUN_100780a0 / FUN_100780d0 as the action packet's u32 @0x0C (c2s 0x01A
  action 0): 1 releases a trust (LSB checks the trust's master).
- Ours (client/combat.rs): all of the above except Dig's zone and ground tests and Sprint's Scout swap.
